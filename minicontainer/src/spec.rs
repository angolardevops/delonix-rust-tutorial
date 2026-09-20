//! Subconjunto da OCI **runtime-spec** (`config.json`).
//!
//! Regra de ouro (herdada do delonix): **um campo que o cliente escreve e o sistema ignora
//! é pior do que um campo que não existe.** Por isso [`Spec::validate`] recusa, pelo nome,
//! tudo o que este runtime não implementa.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};

// region: spec-structs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub oci_version: String,
    pub process: Process,
    pub root: Root,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<Mount>,
    #[serde(default)]
    pub linux: Linux,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    #[serde(default)]
    pub terminal: bool,
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default = "root_dir")]
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    pub path: PathBuf,
    #[serde(default)]
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mount {
    pub destination: PathBuf,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Linux {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<Resources>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub namespaces: Vec<Namespace>,
    /// Presente só para o podermos RECUSAR (não implementamos seccomp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seccomp: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resources {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Memory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pids: Option<Pids>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Cpu>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Bytes; `-1` (ou ausente) = sem limite.
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pids {
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cpu {
    /// Microssegundos por período.
    pub quota: Option<i64>,
    pub period: Option<u64>,
}
// endregion

fn root_dir() -> String {
    "/".to_owned()
}

/// Namespaces que ESTE runtime cria sempre. Um bundle que peça menos é recusado.
const ALWAYS_ISOLATED: [&str; 5] = ["pid", "mount", "uts", "ipc", "network"];

impl Spec {
    pub fn load(bundle: &Path) -> Result<Self> {
        let path = bundle.join("config.json");
        let bytes = std::fs::read(&path).ctx(|| format!("reading {}", path.display()))?;
        let spec: Self =
            serde_json::from_slice(&bytes).map_err(|source| Error::Json { path: path.clone(), source })?;
        spec.validate()?;
        Ok(spec)
    }

    // region: spec-validate
    /// Fail-closed: recusa o que não sabemos cumprir.
    pub fn validate(&self) -> Result<()> {
        if !self.oci_version.starts_with("1.") {
            return Err(Error::Spec(format!("ociVersion {:?} is not 1.x", self.oci_version)));
        }
        if self.process.args.is_empty() {
            return Err(Error::Spec("process.args must not be empty".into()));
        }
        if !self.process.cwd.starts_with('/') {
            return Err(Error::Spec("process.cwd must be absolute".into()));
        }
        if self.process.terminal {
            return Err(Error::Unsupported("process.terminal=true (no pty support)".into()));
        }
        if self.linux.seccomp.is_some() {
            return Err(Error::Unsupported("linux.seccomp (no filter support)".into()));
        }
        for ns in &self.linux.namespaces {
            if ns.path.is_some() {
                return Err(Error::Unsupported(format!(
                    "linux.namespaces[{}].path (joining an existing namespace)",
                    ns.kind
                )));
            }
        }
        if !self.linux.namespaces.is_empty() {
            for needed in ALWAYS_ISOLATED {
                if !self.linux.namespaces.iter().any(|n| n.kind == needed) {
                    return Err(Error::Unsupported(format!(
                        "linux.namespaces without {needed:?}: this runtime always isolates it"
                    )));
                }
            }
        }
        Ok(())
    }
    // endregion

    /// Caminho absoluto do rootfs (o `root.path` é relativo ao bundle).
    pub fn rootfs(&self, bundle: &Path) -> PathBuf {
        if self.root.path.is_absolute() { self.root.path.clone() } else { bundle.join(&self.root.path) }
    }

    /// Mounts por omissão de um bundle gerado por nós.
    pub fn default_mounts() -> Vec<Mount> {
        let m = |dest: &str, kind: &str, opts: &[&str]| Mount {
            destination: dest.into(),
            kind: Some(kind.into()),
            source: Some(kind.into()),
            options: opts.iter().map(|s| (*s).to_owned()).collect(),
        };
        vec![
            m("/proc", "proc", &["nosuid", "noexec", "nodev"]),
            m("/dev", "tmpfs", &["nosuid", "mode=755"]),
            m("/tmp", "tmpfs", &["nosuid", "nodev", "mode=1777"]),
        ]
    }

    pub fn new_default(args: Vec<String>) -> Self {
        Self {
            oci_version: "1.0.2".into(),
            process: Process {
                terminal: false,
                args,
                env: vec!["PATH=/usr/local/bin:/usr/bin:/bin:/sbin".into(), "TERM=xterm".into()],
                cwd: "/".into(),
            },
            root: Root { path: "rootfs".into(), readonly: false },
            hostname: Some("minicontainer".into()),
            mounts: Self::default_mounts(),
            linux: Linux::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Spec {
        Spec::new_default(vec!["/bin/true".into()])
    }

    #[test]
    fn default_spec_is_valid_and_round_trips() {
        let spec = base();
        spec.validate().unwrap();
        let json = serde_json::to_string(&spec).unwrap();
        let back: Spec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.process.args, spec.process.args);
    }

    #[test]
    fn refuses_what_it_cannot_honour() {
        let mut s = base();
        s.linux.seccomp = Some(serde_json::json!({"defaultAction": "SCMP_ACT_ALLOW"}));
        assert!(matches!(s.validate(), Err(Error::Unsupported(_))));

        let mut s = base();
        s.process.terminal = true;
        assert!(matches!(s.validate(), Err(Error::Unsupported(_))));

        let mut s = base();
        s.linux.namespaces = vec![Namespace { kind: "pid".into(), path: Some("/proc/1/ns/pid".into()) }];
        assert!(matches!(s.validate(), Err(Error::Unsupported(_))));
    }

    #[test]
    fn refuses_a_bundle_that_wants_less_isolation() {
        let mut s = base();
        s.linux.namespaces = vec![Namespace { kind: "pid".into(), path: None }];
        let err = s.validate().unwrap_err();
        assert!(err.to_string().contains("mount"), "{err}");
    }

    #[test]
    fn parses_a_real_runc_style_document() {
        let json = r#"{"ociVersion":"1.0.2","process":{"terminal":false,"args":["sh"],"cwd":"/",
            "env":["A=b"],"user":{"uid":0,"gid":0},"capabilities":{"bounding":["CAP_KILL"]}},
            "root":{"path":"rootfs","readonly":true},"hostname":"x",
            "linux":{"resources":{"memory":{"limit":67108864},"pids":{"limit":32}}}}"#;
        let s: Spec = serde_json::from_str(json).unwrap();
        s.validate().unwrap();
        assert_eq!(s.linux.resources.unwrap().pids.unwrap().limit, 32);
    }
}
