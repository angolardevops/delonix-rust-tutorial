#![allow(clippy::unwrap_used)] // helpers de teste: um pânico é a própria falha
//! Testes de integração: correm o binário `mc` a sério.
//! Saltam (com aviso) quando o host não permite user namespaces ou não há busybox estático —
//! um teste que «passa» sem ter corrido nada é pior do que um que diz que saltou.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    bundle: PathBuf,
}

fn setup() -> Option<Env> {
    let bb = which("busybox")?;
    let userns_ok = Command::new("unshare").args(["-Ur", "true"]).status().is_ok_and(|s| s.success());
    if !userns_ok {
        eprintln!("SKIP: user namespaces indisponíveis");
        return None;
    }
    let tmp = tempfile::tempdir().unwrap();
    let bundle = tmp.path().join("bundle");
    let rootfs = bundle.join("rootfs");
    for d in ["bin", "proc", "dev", "tmp", "etc"] {
        std::fs::create_dir_all(rootfs.join(d)).unwrap();
    }
    std::fs::copy(&bb, rootfs.join("bin/busybox")).unwrap();
    for a in ["sh", "cat", "hostname", "sleep", "touch", "echo"] {
        std::os::unix::fs::symlink("busybox", rootfs.join("bin").join(a)).unwrap();
    }
    Some(Env { root: tmp.path().join("state"), _tmp: tmp, bundle })
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

impl Env {
    fn mc(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mc")).args(args).env("MC_ROOT", &self.root).output().unwrap()
    }
    fn spec(&self, script: &str) {
        let out = self.mc(&["spec", self.bundle.to_str().unwrap(), "--", "/bin/sh", "-c", script]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    fn run(&self, id: &str) -> Output {
        self.mc(&["run", id, "-b", self.bundle.to_str().unwrap()])
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn runs_as_pid_1_with_its_own_hostname() {
    let Some(env) = setup() else { return };
    env.spec("echo pid=$$ host=$(hostname)");
    let out = env.run("t-pid");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout(&out).trim(), "pid=1 host=minicontainer");
}

#[test]
fn propagates_the_exit_code_of_the_workload() {
    let Some(env) = setup() else { return };
    env.spec("exit 7");
    assert_eq!(env.run("t-exit").status.code(), Some(7));
}

#[test]
fn a_missing_binary_exits_127_like_a_shell() {
    let Some(env) = setup() else { return };
    let out = env.mc(&["spec", env.bundle.to_str().unwrap(), "--", "/bin/does-not-exist"]);
    assert!(out.status.success());
    assert_eq!(env.run("t-127").status.code(), Some(127));
}

#[test]
fn drops_capabilities_to_the_oci_default_set() {
    let Some(env) = setup() else { return };
    env.spec("cat /proc/self/status");
    let text = stdout(&env.run("t-caps"));
    let bnd = text.lines().find_map(|l| l.strip_prefix("CapBnd:")).unwrap().trim().to_owned();
    assert_eq!(bnd, "00000000a80425fb", "bounding set != OCI default");
}

#[test]
fn readonly_root_is_enforced_but_tmpfs_is_writable() {
    let Some(env) = setup() else { return };
    env.spec("touch /x 2>/dev/null && echo root-rw || echo root-ro; touch /tmp/x && echo tmp-rw");
    let cfg = env.bundle.join("config.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
    v["root"]["readonly"] = true.into();
    std::fs::write(&cfg, serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(stdout(&env.run("t-ro")).trim(), "root-ro\ntmp-rw");
}

#[test]
fn lifecycle_create_start_kill_delete_with_stable_error_classes() {
    let Some(env) = setup() else { return };
    env.spec("sleep 30");
    let b = env.bundle.to_str().unwrap();
    assert!(env.mc(&["create", "life", "-b", b]).status.success());
    assert!(stdout(&env.mc(&["state", "life"])).contains("\"created\""));
    // duplicado → 5 (conflito); inexistente → 4 (não existe)
    assert_eq!(env.mc(&["create", "life", "-b", b]).status.code(), Some(5));
    assert_eq!(env.mc(&["state", "nope"]).status.code(), Some(4));
    // delete sem --force de um container vivo → recusa
    assert_eq!(env.mc(&["delete", "life"]).status.code(), Some(5));
    assert!(env.mc(&["start", "life"]).status.success());
    assert!(env.mc(&["kill", "life"]).status.success());
    std::thread::sleep(std::time::Duration::from_millis(400));
    let st = stdout(&env.mc(&["state", "life"]));
    assert!(st.contains("\"stopped\"") && st.contains("137"), "{st}");
    assert!(env.mc(&["delete", "life"]).status.success());
}

#[test]
fn a_bundle_asking_for_seccomp_is_refused_not_ignored() {
    let Some(env) = setup() else { return };
    env.spec("true");
    let cfg = env.bundle.join("config.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
    v["linux"]["seccomp"] = serde_json::json!({"defaultAction": "SCMP_ACT_ALLOW"});
    std::fs::write(&cfg, serde_json::to_vec(&v).unwrap()).unwrap();
    let out = env.run("t-seccomp");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("seccomp"));
}
