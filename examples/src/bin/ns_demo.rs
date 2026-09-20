//! Capítulo 3 — namespaces a sério. Corre como binário (processo de uma só thread), porque
//! `unshare(CLONE_NEWUSER)` recusa processos com várias threads — e o harness de testes tem várias.
//!
//! `cargo run -p examples --bin ns_demo`

use nix::sched::{CloneFlags, unshare};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, fork, gethostname, getpid, getuid, sethostname};
use std::fs;

fn main() {
    let host_before = gethostname().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
    let (uid, gid) = (getuid(), nix::unistd::getgid());

    // 1. Novo user namespace (sem privilégio) + UTS + PID.
    if let Err(e) = unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWUTS | CloneFlags::CLONE_NEWPID) {
        eprintln!("SKIP: unshare falhou ({e}) — user namespaces desactivados neste host?");
        std::process::exit(0);
    }
    // 2. Mapear o nosso uid para root DENTRO (ler o uid ANTES do unshare!).
    fs::write("/proc/self/setgroups", "deny").expect("setgroups");
    fs::write("/proc/self/uid_map", format!("0 {uid} 1")).expect("uid_map");
    fs::write("/proc/self/gid_map", format!("0 {gid} 1")).expect("gid_map");

    // 3. O PID namespace só vale para os FILHOS: fork → o filho é o PID 1.
    // SAFETY: processo de uma só thread; o filho só usa chamadas simples e sai com `exit`.
    match unsafe { fork() }.expect("fork") {
        ForkResult::Child => {
            sethostname("dentro-do-ns").expect("sethostname");
            println!(
                "child: pid={} uid={} hostname={}",
                getpid(),
                getuid(),
                gethostname().expect("gethostname").to_string_lossy()
            );
            std::process::exit(0);
        }
        ForkResult::Parent { child } => {
            assert!(matches!(waitpid(child, None), Ok(WaitStatus::Exited(_, 0))));
            let host_after = gethostname().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
            println!(
                "parent: hostname antes={host_before:?} depois={host_after:?} (inalterado: {})",
                host_before == host_after
            );
        }
    }
}
