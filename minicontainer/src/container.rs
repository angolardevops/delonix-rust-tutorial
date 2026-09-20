//! Ciclo de vida OCI: `create` → `start` → (`kill`) → `delete`.
//!
//! Processos (sem daemon; um supervisor por container, como o supervisor do delonix):
//!
//! ```text
//! mc create ──fork──▶ supervisor ──unshare(user,pid,uts,ipc,net)──fork──▶ init (PID 1 do container)
//!    ▲                    │  ◀── relatório de arranque (pipe) ──────────────┘   mount ns, pivot_root,
//!    └── relatório (pipe)─┘                                                     bloqueia no FIFO
//! mc start ──escreve 1 byte no FIFO──▶ init faz exec do processo do utilizador
//! ```

use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use nix::fcntl::OFlag;
use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::sched::{CloneFlags, unshare};
use nix::sys::signal::{Signal, kill};
use nix::sys::stat::Mode;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, Pid, chdir, fork, mkfifo, pipe, pivot_root, sethostname, setsid};

use crate::cgroup::Cgroup;
use crate::error::{Error, IoContext, Result};
use crate::fsutil::resolve_in_root;
use crate::spec::{Mount, Spec};
use crate::state::{State, Status, Store, reconcile};

/// Capabilities que o container mantém (o conjunto por omissão da runtime-spec).
const KEEP_CAPS: [u32; 14] = [0, 1, 3, 4, 5, 6, 7, 8, 10, 13, 18, 27, 29, 31];

// region: stdio
/// Para onde vai o stdio do container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stdio {
    /// Herda o do chamador (`mc run`: o utilizador vê a saída).
    Inherit,
    /// Vai para `<estado>/output.log` (`mc create`): o container sobrevive ao comando que o criou,
    /// e um chamador que capture a saída por pipe não fica preso à espera de um EOF que só
    /// chegaria quando o container morresse.
    Log,
}
// endregion

// region: create
pub fn create(store: &Store, id: &str, bundle: &Path, stdio: Stdio) -> Result<()> {
    let bundle = bundle.canonicalize().ctx(|| format!("bundle {}", bundle.display()))?;
    let spec = Spec::load(&bundle)?;
    let rootfs = spec.rootfs(&bundle).canonicalize().ctx(|| "rootfs".to_string())?;

    store.create(&State {
        oci_version: "1.0.2".into(),
        id: id.into(),
        status: Status::Creating,
        pid: 0,
        bundle: bundle.clone(),
        exit_code: None,
        cgroup: None,
        oom_killed: false,
    })?;
    let result = spawn(store, id, &spec, &rootfs, stdio);
    if result.is_err() {
        let _ = store.remove(id); // não deixar meio-container para trás
    }
    result
}
// endregion

// region: spawn
fn spawn(store: &Store, id: &str, spec: &Spec, rootfs: &Path, stdio: Stdio) -> Result<()> {
    let fifo = store.fifo(id)?;
    mkfifo(&fifo, Mode::from_bits_truncate(0o600))?;
    // O cgroup é preparado ANTES do fork: um erro de delegação chega ao utilizador com o remédio.
    let cgroup = Cgroup::create(id, spec.linux.resources.as_ref())?;

    let (rd, wr) = pipe()?;
    // SAFETY: o processo é single-thread neste ponto (nenhuma thread foi lançada), logo
    // `fork` é seguro; o filho só chama funções async-signal-safe até ao `exec`/`_exit`.
    match unsafe { fork() }? {
        ForkResult::Parent { .. } => {
            drop(wr);
            let report = recv_report(rd)?;
            let pid: i32 = report.parse().map_err(|_| Error::Setup(format!("bad report {report:?}")))?;
            let cg = cgroup.as_ref().map(|c| c.path().to_owned());
            store.update(id, |st| {
                st.pid = pid;
                st.cgroup = cg;
                if st.status == Status::Creating {
                    st.status = Status::Created;
                }
                Ok(())
            })
        }
        ForkResult::Child => {
            drop(rd);
            supervise(store, id, spec, rootfs, &fifo, cgroup, wr, stdio)
        }
    }
}
// endregion

/// stdin ← /dev/null; stdout/stderr → ficheiro. Herdado pelo init.
fn detach_stdio(log: &Path) -> Result<()> {
    let null = fs::File::open("/dev/null").ctx(|| "/dev/null".to_string())?;
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .ctx(|| format!("opening {}", log.display()))?;
    nix::unistd::dup2_stdin(&null)?;
    nix::unistd::dup2_stdout(&file)?;
    nix::unistd::dup2_stderr(&file)?;
    Ok(())
}

// region: supervise
/// Corre no processo supervisor. Nunca regressa.
#[allow(clippy::too_many_arguments)]
fn supervise(
    store: &Store,
    id: &str,
    spec: &Spec,
    rootfs: &Path,
    fifo: &Path,
    cg: Option<Cgroup>,
    out: OwnedFd,
    stdio: Stdio,
) -> ! {
    let _ = setsid();
    if stdio == Stdio::Log
        && let Err(e) = detach_stdio(&store.dir(id).unwrap_or_default().join("output.log"))
    {
        die(out, &e);
    }
    let (irx, itx) = match pipe() {
        Ok(p) => p,
        Err(e) => die(out, &Error::Sys(e)),
    };
    // Ler os ids ANTES do unshare: depois dele `getuid()` devolve o uid «overflow» (65534)
    // e o `uid_map` seria recusado com EPERM.
    let (uid, gid) = (nix::unistd::getuid(), nix::unistd::getgid());
    let prep = || -> Result<()> {
        unshare(
            CloneFlags::CLONE_NEWUSER
                | CloneFlags::CLONE_NEWPID
                | CloneFlags::CLONE_NEWUTS
                | CloneFlags::CLONE_NEWIPC
                | CloneFlags::CLONE_NEWNET,
        )?;
        map_ids(uid, gid)?;
        loopback_up()
    };
    if let Err(e) = prep() {
        die(out, &e);
    }
    // SAFETY: continuamos single-thread; ver `spawn`.
    let init = match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            drop(irx);
            let err = match container_init(spec, rootfs, fifo, &itx) {
                Ok(never) => match never {},
                Err(e) => e,
            };
            send(&itx, format!("E{err}").as_bytes());
            std::process::exit(1);
        }
        Ok(ForkResult::Parent { child }) => child,
        Err(e) => die(out, &Error::Sys(e)),
    };
    drop(itx);
    if let Some(c) = &cg
        && let Err(e) = c.attach(init.as_raw())
    {
        let _ = kill(init, Signal::SIGKILL);
        die(out, &e);
    }
    match recv_report(irx) {
        Ok(_) => {
            send(&out, format!("K{}", init.as_raw()).as_bytes());
            drop(out);
        }
        Err(e) => {
            let _ = waitpid(init, None);
            die(out, &e);
        }
    }

    // Espera pelo fim do container e regista o resultado.
    let code = match waitpid(init, None) {
        Ok(WaitStatus::Exited(_, c)) => c,
        Ok(WaitStatus::Signaled(_, sig, _)) => 128 + sig as i32,
        _ => 255,
    };
    let oom = cg.as_ref().is_some_and(|c| c.oom_kills() > 0);
    let _ = store.update(id, |st| {
        st.status = Status::Stopped;
        st.exit_code = Some(code);
        st.oom_killed = oom;
        Ok(())
    });
    if let Some(c) = &cg {
        Cgroup::remove(c.path());
    }
    std::process::exit(0)
}
// endregion

/// Escrita best-effort num pipe de relatório (o leitor pode já ter desistido).
fn send(fd: &OwnedFd, msg: &[u8]) {
    let _ = nix::unistd::write(fd, msg);
}

fn die(out: OwnedFd, e: &Error) -> ! {
    send(&out, format!("E{e}").as_bytes());
    std::process::exit(1)
}

// region: recv
/// Lê o relatório: `K<texto>` = ok, `E<mensagem>` = erro, EOF = o filho morreu sem dizer nada.
///
/// UMA leitura, não «até ao EOF»: o emissor pode manter o pipe aberto até ao `exec`
/// (o init só o fecha depois do `start`), e esperar por EOF bloquearia o `create` para sempre.
fn recv_report(fd: OwnedFd) -> Result<String> {
    let mut raw = [0u8; 4096];
    let n = fs::File::from(fd).read(&mut raw).ctx(|| "reading start-up report".to_string())?;
    let buf = String::from_utf8_lossy(&raw[..n]).into_owned();
    match buf.split_at_checked(1) {
        Some(("K", rest)) => Ok(rest.to_owned()),
        Some(("E", msg)) => Err(Error::Setup(msg.to_owned())),
        _ => Err(Error::Setup("container process died during setup".into())),
    }
}
// endregion

// region: map-ids
/// Mapeamento de um só uid/gid (root dentro = o teu utilizador fora). Para vários uids seria
/// preciso `newuidmap` + `/etc/subuid`.
fn map_ids(uid: nix::unistd::Uid, gid: nix::unistd::Gid) -> Result<()> {
    let w = |f: &str, v: String| {
        fs::write(format!("/proc/self/{f}"), v).ctx(|| format!("writing /proc/self/{f}"))
    };
    w("setgroups", "deny".into())?; // obrigatório antes do gid_map sem privilégio
    w("uid_map", format!("0 {uid} 1"))?;
    w("gid_map", format!("0 {gid} 1"))
}
// endregion

// region: loopback
/// Sobe `lo` no netns novo (ioctl SIOCSIFFLAGS). Sem isto `127.0.0.1` não responde.
fn loopback_up() -> Result<()> {
    // SAFETY: `ifreq` é POD de zeros válido; o socket é fechado no fim; nomes ≤ IFNAMSIZ.
    unsafe {
        let sock = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if sock < 0 {
            return Err(Error::Sys(nix::Error::last()));
        }
        let mut ifr: libc::ifreq = std::mem::zeroed();
        for (d, s) in ifr.ifr_name.iter_mut().zip(b"lo\0") {
            *d = *s as libc::c_char;
        }
        ifr.ifr_ifru.ifru_flags = (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_short;
        let rc = libc::ioctl(sock, libc::SIOCSIFFLAGS as _, &ifr);
        let err = nix::Error::last();
        libc::close(sock);
        if rc < 0 { Err(Error::Sys(err)) } else { Ok(()) }
    }
}
// endregion

// region: init
/// PID 1 do container. Só regressa com erro — no sucesso faz `exec`.
fn container_init(
    spec: &Spec,
    rootfs: &Path,
    fifo: &Path,
    report: &OwnedFd,
) -> Result<std::convert::Infallible> {
    unshare(CloneFlags::CLONE_NEWNS)?;
    // Sem isto os nossos mounts propagavam para o host.
    mount(None::<&str>, "/", None::<&str>, MsFlags::MS_REC | MsFlags::MS_PRIVATE, None::<&str>)?;
    // pivot_root exige que o novo root seja um ponto de montagem.
    mount(Some(rootfs), rootfs, None::<&str>, MsFlags::MS_BIND | MsFlags::MS_REC, None::<&str>)?;

    let mut dev_mounted = false;
    for m in &spec.mounts {
        apply_mount(rootfs, m)?;
        dev_mounted |= m.destination == Path::new("/dev");
    }
    if dev_mounted {
        setup_dev(rootfs)?;
    }

    // Aberto ANTES do pivot: depois dele o caminho do FIFO já não existe.
    // O_RDWR não bloqueia na abertura; o `read` bloqueia até `start` escrever.
    let fifo_fd = nix::fcntl::open(fifo, OFlag::O_RDWR | OFlag::O_CLOEXEC, Mode::empty())?;

    chdir(rootfs)?;
    pivot_root(".", ".")?; // truque: raiz nova e antiga empilhadas no mesmo sítio…
    umount2(".", MntFlags::MNT_DETACH)?; // …e a antiga desmonta-se.
    chdir("/")?;
    if spec.root.readonly {
        remount_ro("/")?;
    }
    if let Some(h) = &spec.hostname {
        sethostname(h)?;
    }
    drop_capabilities()?;
    // SAFETY: prctl com argumentos inteiros constantes.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(Error::Sys(nix::Error::last()));
    }

    // Tudo pronto: diz ao supervisor e espera pelo `start`.
    send(report, b"K");
    let mut byte = [0u8; 1];
    nix::unistd::read(&fifo_fd, &mut byte)?;
    drop(fifo_fd);

    chdir(spec.process.cwd.as_str())?;
    let args: Vec<CString> = spec.process.args.iter().map(|a| cstr(a)).collect::<Result<_>>()?;
    let env: Vec<CString> = spec.process.env.iter().map(|a| cstr(a)).collect::<Result<_>>()?;
    let err = nix::unistd::execvpe(&args[0], &args, &env).unwrap_err();
    // Já depois do `start`: o relatório fechou; diz-se no stderr e sai-se como a shell (127).
    eprintln!("mc: cannot exec {:?}: {err}", spec.process.args[0]);
    std::process::exit(if err == nix::Error::ENOENT { 127 } else { 126 });
}
// endregion

fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::Spec(format!("NUL byte in {s:?}")))
}

// region: caps
fn drop_capabilities() -> Result<()> {
    let last: u32 = fs::read_to_string("/proc/sys/kernel/cap_last_cap")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(40);
    for cap in (0..=last).filter(|c| !KEEP_CAPS.contains(c)) {
        // SAFETY: prctl com inteiros; EINVAL para caps desconhecidas é tolerado.
        let rc = unsafe { libc::prctl(libc::PR_CAPBSET_DROP, libc::c_ulong::from(cap), 0, 0, 0) };
        if rc != 0 && nix::Error::last() != nix::Error::EINVAL {
            return Err(Error::Sys(nix::Error::last()));
        }
    }
    Ok(())
}
// endregion

/// Flags «trancadas» que o mount herdado do host impõe: um remount sem elas dá EPERM em userns.
fn inherited_flags(path: &str) -> MsFlags {
    use nix::sys::statvfs::{FsFlags, statvfs};
    let mut out = MsFlags::empty();
    if let Ok(v) = statvfs(path) {
        let f = v.flags();
        if f.contains(FsFlags::ST_NOSUID) {
            out |= MsFlags::MS_NOSUID
        }
        if f.contains(FsFlags::ST_NODEV) {
            out |= MsFlags::MS_NODEV
        }
        if f.contains(FsFlags::ST_NOEXEC) {
            out |= MsFlags::MS_NOEXEC
        }
    }
    out
}

fn remount_ro(path: &str) -> Result<()> {
    let flags = MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY | inherited_flags(path);
    mount(None::<&str>, path, None::<&str>, flags, None::<&str>)?;
    Ok(())
}

// region: apply-mount
fn apply_mount(rootfs: &Path, m: &Mount) -> Result<()> {
    let target = resolve_in_root(rootfs, &m.destination, true)?;
    let (mut flags, mut ro) = (MsFlags::empty(), false);
    let mut data: Vec<&str> = Vec::new();
    for o in &m.options {
        match o.as_str() {
            "ro" => ro = true,
            "nosuid" => flags |= MsFlags::MS_NOSUID,
            "nodev" => flags |= MsFlags::MS_NODEV,
            "noexec" => flags |= MsFlags::MS_NOEXEC,
            "bind" => flags |= MsFlags::MS_BIND,
            "rbind" => flags |= MsFlags::MS_BIND | MsFlags::MS_REC,
            "rw" | "relatime" | "strictatime" => {}
            other => data.push(other),
        }
    }
    let kind = m.kind.as_deref().unwrap_or("bind");
    let bind = kind == "bind" || flags.contains(MsFlags::MS_BIND);
    match kind {
        "proc" | "tmpfs" | "bind" => {}
        other => return Err(Error::Unsupported(format!("mount type {other:?}"))),
    }
    let source = m.source.as_deref();
    if bind {
        let src = source.ok_or_else(|| Error::Spec("bind mount without source".into()))?;
        if src.is_file() {
            // o alvo de um bind de ficheiro tem de ser um ficheiro
            fs::remove_dir(&target).ok();
            fs::File::create(&target).ctx(|| format!("creating {}", target.display()))?;
        }
        mount(Some(src), &target, None::<&str>, flags | MsFlags::MS_BIND, None::<&str>)?;
        if ro || !(flags - MsFlags::MS_BIND - MsFlags::MS_REC).is_empty() {
            let t = target.to_string_lossy();
            let mut f = MsFlags::MS_BIND | MsFlags::MS_REMOUNT | flags | inherited_flags(&t);
            if ro {
                f |= MsFlags::MS_RDONLY
            }
            mount(None::<&str>, &target, None::<&str>, f, None::<&str>)?;
        }
    } else {
        let data = data.join(",");
        if ro {
            flags |= MsFlags::MS_RDONLY
        }
        mount(source, &target, Some(kind), flags, if data.is_empty() { None } else { Some(data.as_str()) })?;
    }
    Ok(())
}
// endregion

// region: setup-dev
/// Sem root não há `mknod`: os dispositivos básicos são *bind-mounts* dos do host.
fn setup_dev(rootfs: &Path) -> Result<()> {
    let dev = resolve_in_root(rootfs, Path::new("/dev"), false)?; // sem symlinks pelo caminho
    for name in ["null", "zero", "full", "random", "urandom", "tty"] {
        let target = dev.join(name);
        fs::File::create(&target).ctx(|| format!("creating {}", target.display()))?;
        mount(
            Some(&*PathBuf::from("/dev").join(name)),
            &target,
            None::<&str>,
            MsFlags::MS_BIND,
            None::<&str>,
        )?;
    }
    for (link, to) in [
        ("fd", "/proc/self/fd"),
        ("stdin", "/proc/self/fd/0"),
        ("stdout", "/proc/self/fd/1"),
        ("stderr", "/proc/self/fd/2"),
    ] {
        std::os::unix::fs::symlink(to, dev.join(link)).ctx(|| format!("symlink /dev/{link}"))?;
    }
    Ok(())
}
// endregion

// region: start
pub fn start(store: &Store, id: &str) -> Result<()> {
    let st = reconcile(store.load(id)?);
    if st.status != Status::Created {
        return Err(Error::WrongState { id: id.into(), status: st.status.to_string(), expected: "created" });
    }
    let mut fifo =
        fs::OpenOptions::new().write(true).open(store.fifo(id)?).ctx(|| "opening start fifo".to_string())?;
    fifo.write_all(&[1]).ctx(|| "signalling start".to_string())?;
    // Compare-and-set: se o container já acabou (comando curto), o supervisor ganhou — não desfazer.
    store.update(id, |s| {
        if s.status == Status::Created {
            s.status = Status::Running;
        }
        Ok(())
    })
}
// endregion

pub fn kill_container(store: &Store, id: &str, sig: Signal) -> Result<()> {
    let st = reconcile(store.load(id)?);
    if !matches!(st.status, Status::Created | Status::Running) {
        return Err(Error::WrongState {
            id: id.into(),
            status: st.status.to_string(),
            expected: "created or running",
        });
    }
    // O init de um pidns só recebe sinais que tenha tratado — excepto SIGKILL vindo de fora.
    kill(Pid::from_raw(st.pid), sig)?;
    Ok(())
}

pub fn delete(store: &Store, id: &str, force: bool) -> Result<()> {
    let st = reconcile(store.load(id)?);
    if matches!(st.status, Status::Created | Status::Running) {
        if !force {
            return Err(Error::WrongState {
                id: id.into(),
                status: st.status.to_string(),
                expected: "stopped (or use --force)",
            });
        }
        let _ = kill(Pid::from_raw(st.pid), Signal::SIGKILL);
        wait_stopped(store, id);
    }
    if let Some(cg) = store.load(id)?.cgroup {
        Cgroup::remove(Cgroup::from_path(cg).path());
    }
    store.remove(id)
}

// region: wait
/// Espera (com tecto) que o supervisor registe o fim.
///
/// Espera pelo estado **persistido** `stopped`, não por «o PID morreu»: entre as duas coisas o
/// supervisor ainda tem de gravar o exit code, e ler nessa janela devolvia um código em falta.
/// Só se o supervisor desaparecer sem gravar (foi morto) é que se desiste, após uma folga.
pub fn wait_stopped(store: &Store, id: &str) -> Option<i32> {
    let mut dead_polls = 0;
    for _ in 0..1500 {
        let st = store.load(id).ok()?;
        if st.status == Status::Stopped {
            return st.exit_code;
        }
        if reconcile(st).status == Status::Stopped {
            dead_polls += 1;
            if dead_polls > 100 {
                return None; // 2 s sem o supervisor gravar: morreu a meio
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    None
}
// endregion
