//! cgroup v2 rootless: só funciona debaixo de uma subárvore **delegada** ao teu utilizador.
//!
//! O que a medição do delonix ensinou: numa sessão SSH normal os limites são inertes, porque
//! o scope da sessão é irmão de `user@UID.service` e não filho. Aqui, em vez de fingir que
//! aplicámos um limite, **recusamos** se não o conseguirmos aplicar. Remédio:
//! `systemd-run --user --scope -p Delegate=yes -- mc run ...`

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, IoContext, Result};
use crate::spec::Resources;

const ROOT: &str = "/sys/fs/cgroup";

#[derive(Debug)]
pub struct Cgroup {
    path: PathBuf,
}

// region: limits
/// Limites já traduzidos para os ficheiros do cgroup v2 (função pura → testável).
pub fn limit_files(res: &Resources) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Some(limit) = res.memory.as_ref().and_then(|m| m.limit) {
        out.push(("memory.max", if limit < 0 { "max".to_owned() } else { limit.to_string() }));
        // Sem isto o kernel «cumpre» o limite empurrando páginas para swap.
        out.push(("memory.swap.max", "0".to_owned()));
    }
    if let Some(p) = &res.pids {
        out.push(("pids.max", if p.limit < 0 { "max".to_owned() } else { p.limit.to_string() }));
    }
    if let Some(quota) = res.cpu.as_ref().and_then(|c| c.quota.filter(|q| *q > 0)) {
        let period = res.cpu.as_ref().and_then(|c| c.period).unwrap_or(100_000);
        out.push(("cpu.max", format!("{quota} {period}")));
    }
    out
}
// endregion

fn wanted_controllers(files: &[(&str, String)]) -> Vec<&'static str> {
    let mut c = Vec::new();
    for (name, _) in files {
        let ctl = match *name {
            "memory.max" | "memory.swap.max" => "memory",
            "pids.max" => "pids",
            _ => "cpu",
        };
        if !c.contains(&ctl) {
            c.push(ctl);
        }
    }
    c
}

fn own_cgroup() -> Result<PathBuf> {
    let text = fs::read_to_string("/proc/self/cgroup").ctx(|| "reading /proc/self/cgroup".into())?;
    let rel = text
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or_else(|| Error::Cgroup("not a cgroup v2 host".into()))?;
    Ok(Path::new(ROOT).join(rel.trim_start_matches('/')))
}

impl Cgroup {
    // region: cg-create
    /// `Ok(None)` quando o bundle não pede limites — não se mexe em cgroups sem necessidade.
    pub fn create(id: &str, res: Option<&Resources>) -> Result<Option<Self>> {
        let files = res.map(limit_files).unwrap_or_default();
        if files.is_empty() {
            return Ok(None);
        }
        let hint = "run under a delegated scope: systemd-run --user --scope -p Delegate=yes -- mc ...";
        let parent = own_cgroup()?;

        // Regra «no internal processes»: para activar controladores em `parent` ele não pode
        // ter processos directos. Mudamo-nos para um filho `mc-mgr` (o delonix chama-lhe `dlx-mgr`).
        let mgr = parent.join("mc-mgr");
        fs::create_dir_all(&mgr)
            .map_err(|e| Error::Cgroup(format!("cannot create {}: {e} ({hint})", mgr.display())))?;
        fs::write(mgr.join("cgroup.procs"), b"0")
            .map_err(|e| Error::Cgroup(format!("cannot move into {}: {e} ({hint})", mgr.display())))?;

        let available = fs::read_to_string(parent.join("cgroup.controllers")).unwrap_or_default();
        let enable: Vec<String> = wanted_controllers(&files)
            .into_iter()
            .filter(|c| available.split_whitespace().any(|a| a == *c))
            .map(|c| format!("+{c}"))
            .collect();
        let missing = wanted_controllers(&files).len() - enable.len();
        if missing > 0 {
            return Err(Error::Cgroup(format!(
                "a requested controller is not delegated here (have: {available:?}); {hint}"
            )));
        }
        fs::write(parent.join("cgroup.subtree_control"), enable.join(" "))
            .map_err(|e| Error::Cgroup(format!("cannot enable controllers: {e} ({hint})")))?;

        let leaf = parent.join(format!("mc-{id}"));
        fs::create_dir(&leaf).map_err(|e| Error::Cgroup(format!("creating {}: {e}", leaf.display())))?;
        for (name, value) in &files {
            fs::write(leaf.join(name), value)
                .map_err(|e| Error::Cgroup(format!("writing {name}={value}: {e}")))?;
        }
        Ok(Some(Self { path: leaf }))
    }
    // endregion

    pub fn attach(&self, pid: i32) -> Result<()> {
        fs::write(self.path.join("cgroup.procs"), pid.to_string())
            .ctx(|| format!("attaching pid {pid} to {}", self.path.display()))
    }

    /// `memory.events` → quantas vezes o kernel matou por OOM (o delonix lê isto ao vivo,
    /// porque o cgroup desaparece com o container).
    pub fn oom_kills(&self) -> u64 {
        fs::read_to_string(self.path.join("memory.events"))
            .ok()
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("oom_kill ")?.trim().parse().ok()))
            .unwrap_or(0)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Só remove com o cgroup vazio; um erro aqui não deve esconder o resultado do container.
    pub fn remove(path: &Path) {
        let _ = fs::remove_dir(path);
    }

    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Cpu, Memory, Pids};

    #[test]
    fn translates_limits_to_v2_files() {
        let res = Resources {
            memory: Some(Memory { limit: Some(64 << 20) }),
            pids: Some(Pids { limit: 32 }),
            cpu: Some(Cpu { quota: Some(50_000), period: Some(100_000) }),
        };
        let f = limit_files(&res);
        assert!(f.contains(&("memory.max", "67108864".to_owned())));
        assert!(f.contains(&("memory.swap.max", "0".to_owned())));
        assert!(f.contains(&("pids.max", "32".to_owned())));
        assert!(f.contains(&("cpu.max", "50000 100000".to_owned())));
    }

    #[test]
    fn negative_means_unlimited_and_no_limits_means_no_files() {
        let res = Resources { memory: Some(Memory { limit: Some(-1) }), ..Default::default() };
        assert_eq!(limit_files(&res)[0], ("memory.max", "max".to_owned()));
        assert!(limit_files(&Resources::default()).is_empty());
    }
}
