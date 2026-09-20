//! Estado persistido — **sem daemon**: o estado vive em disco, não num processo.
//!
//! Escritas com `flock` + rename atómico. Sem lock, dois processos (o supervisor a gravar
//! `stopped` e o `start` a gravar `running`) perdiam uma das escritas — o mesmo defeito que o
//! delonix corrigiu no `Store::update`.

use std::fs;
use std::path::{Path, PathBuf};

use nix::fcntl::{Flock, FlockArg};
use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsutil::{valid_id, write_atomic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Creating,
    Created,
    Running,
    Stopped,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Creating => "creating",
            Self::Created => "created",
            Self::Running => "running",
            Self::Stopped => "stopped",
        };
        f.write_str(s)
    }
}

/// Os campos `oci_version`..`bundle` são o «state» exigido pela runtime-spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub oci_version: String,
    pub id: String,
    pub status: Status,
    pub pid: i32,
    pub bundle: PathBuf,
    // --- extensões nossas ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cgroup: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oom_killed: bool,
}

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// `MC_ROOT` > `$XDG_RUNTIME_DIR/minicontainer` > `/tmp/minicontainer-<uid>`.
    pub fn open() -> Result<Self> {
        let root = std::env::var_os("MC_ROOT").map(PathBuf::from).unwrap_or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR").map_or_else(
                || PathBuf::from(format!("/tmp/minicontainer-{}", nix::unistd::getuid())),
                |d| Path::new(&d).join("minicontainer"),
            )
        });
        fs::create_dir_all(&root).ctx(|| format!("creating {}", root.display()))?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .ctx(|| format!("chmod {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn dir(&self, id: &str) -> Result<PathBuf> {
        valid_id(id)?;
        Ok(self.root.join(id))
    }

    pub fn fifo(&self, id: &str) -> Result<PathBuf> {
        Ok(self.dir(id)?.join("start.fifo"))
    }

    pub fn create(&self, state: &State) -> Result<()> {
        let dir = self.dir(&state.id)?;
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(Error::Conflict(state.id.clone()));
            }
            Err(e) => return Err(Error::io(format!("creating {}", dir.display()), e)),
        }
        self.write(state)
    }

    fn write(&self, state: &State) -> Result<()> {
        let dir = self.dir(&state.id)?;
        let json =
            serde_json::to_vec_pretty(state).map_err(|source| Error::Json { path: dir.clone(), source })?;
        write_atomic(&dir.join("state.json"), &json)
    }

    pub fn load(&self, id: &str) -> Result<State> {
        let path = self.dir(id)?.join("state.json");
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| Error::Json { path, source }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::NotFound(id.into())),
            Err(e) => Err(Error::io(format!("reading {}", path.display()), e)),
        }
    }

    // region: state-update
    /// Read-modify-write sob `flock`. A closure decide o que fazer com o estado ACTUAL.
    pub fn update<T>(&self, id: &str, f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let dir = self.dir(id)?;
        let lock_file =
            fs::OpenOptions::new().create(true).append(true).open(dir.join("lock")).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::NotFound(id.into())
                } else {
                    Error::io("opening lock", e)
                }
            })?;
        let _guard = Flock::lock(lock_file, FlockArg::LockExclusive).map_err(|(_, e)| Error::Sys(e))?;
        let mut state = self.load(id)?;
        let out = f(&mut state)?;
        self.write(&state)?;
        Ok(out)
    }
    // endregion

    pub fn list(&self) -> Result<Vec<State>> {
        let mut out = Vec::new();
        for e in fs::read_dir(&self.root).ctx(|| format!("reading {}", self.root.display()))?.flatten() {
            if let Some(id) = e.file_name().to_str()
                && let Ok(st) = self.load(id)
            {
                out.push(st);
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        fs::remove_dir_all(self.dir(id)?).ctx(|| format!("removing state of {id}"))
    }
}

// region: reconcile
/// O estado em disco pode mentir (o supervisor morreu, a máquina reiniciou): reconcilia com a
/// realidade. Nunca se confia só no ficheiro — a mesma lição do `reconcile_status` do delonix.
pub fn reconcile(mut st: State) -> State {
    if matches!(st.status, Status::Created | Status::Running)
        && nix::sys::signal::kill(nix::unistd::Pid::from_raw(st.pid), None).is_err()
    {
        st.status = Status::Stopped;
    }
    st
}
// endregion

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store { root: d.path().to_owned() };
        (d, s)
    }

    fn st(id: &str) -> State {
        State {
            oci_version: "1.0.2".into(),
            id: id.into(),
            status: Status::Created,
            pid: 1,
            bundle: "/b".into(),
            exit_code: None,
            cgroup: None,
            oom_killed: false,
        }
    }

    #[test]
    fn create_twice_is_a_conflict_and_missing_is_not_found() {
        let (_d, s) = store();
        s.create(&st("a")).unwrap();
        assert!(matches!(s.create(&st("a")), Err(Error::Conflict(_))));
        assert!(matches!(s.load("zzz"), Err(Error::NotFound(_))));
    }

    // region: concurrent-test
    #[test]
    fn concurrent_updates_do_not_lose_writes() {
        let (_d, s) = store();
        let mut init = st("c");
        init.pid = 0;
        s.create(&init).unwrap();
        let s = std::sync::Arc::new(s);
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let s = s.clone();
                std::thread::spawn(move || {
                    s.update("c", |st| {
                        let v = st.pid;
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        st.pid = v + 1;
                        Ok(())
                    })
                    .unwrap();
                })
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
        assert_eq!(s.load("c").unwrap().pid, 16);
    }
    // endregion

    #[test]
    fn a_dead_pid_is_reported_stopped() {
        let mut s = st("x");
        s.pid = i32::MAX - 1;
        s.status = Status::Running;
        assert_eq!(reconcile(s).status, Status::Stopped);
    }
}
