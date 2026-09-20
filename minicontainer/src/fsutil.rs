//! Utilitários de sistema de ficheiros com foco em **não sair do rootfs**.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, IoContext, Result};

/// Junta `rel` a `root` recusando `..`, prefixos e — sobretudo — **symlinks** pelo caminho.
///
/// Porque não basta `root.join(rel)`: uma imagem pode plantar `etc -> /` e um bind mount
/// para `etc/passwd` acabaria no `/` do *host*. Resolvemos componente a componente e
/// recusamos qualquer symlink (é o que o delonix faz em `safe_bind_target`).
/// Com `create`, os directórios que faltam são criados.
pub fn resolve_in_root(root: &Path, rel: &Path, create: bool) -> Result<PathBuf> {
    let mut cur = root.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                cur.push(name);
                match fs::symlink_metadata(&cur) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        return Err(Error::UnsafePath(cur));
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                        fs::create_dir(&cur).ctx(|| format!("creating {}", cur.display()))?;
                    }
                    Err(e) => return Err(Error::io(format!("stat {}", cur.display()), e)),
                }
            }
            Component::ParentDir | Component::Prefix(_) => {
                return Err(Error::UnsafePath(rel.to_path_buf()));
            }
        }
    }
    Ok(cur)
}

/// Escrita atómica: ficheiro temporário no MESMO directório + `rename`.
/// Um leitor concorrente vê o ficheiro antigo inteiro ou o novo inteiro — nunca metade.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(".{}.tmp{}", file_name(path), std::process::id()));
    let mut f = fs::File::create(&tmp).ctx(|| format!("creating {}", tmp.display()))?;
    f.write_all(bytes).ctx(|| format!("writing {}", tmp.display()))?;
    f.sync_all().ctx(|| format!("fsync {}", tmp.display()))?;
    fs::rename(&tmp, path).ctx(|| format!("renaming to {}", path.display()))
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| "state".into(), |n| n.to_string_lossy().into_owned())
}

/// Identificadores de container: entram em caminhos de disco e de cgroup, logo lista branca.
pub fn valid_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && !id.starts_with(['-', '.'])
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok { Ok(()) } else { Err(Error::Spec(format!("invalid container id {id:?}"))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_parent_dir_and_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(resolve_in_root(dir.path(), Path::new("../etc"), false), Err(Error::UnsafePath(_))));
        std::os::unix::fs::symlink("/", dir.path().join("etc")).unwrap();
        assert!(matches!(
            resolve_in_root(dir.path(), Path::new("/etc/passwd"), true),
            Err(Error::UnsafePath(_))
        ));
    }

    #[test]
    fn creates_missing_directories() {
        let dir = tempfile::tempdir().unwrap();
        let p = resolve_in_root(dir.path(), Path::new("/a/b/c"), true).unwrap();
        assert!(p.is_dir());
    }

    #[test]
    fn ids_are_allow_listed() {
        for bad in ["", "..", "-x", "a/b", "a b", &"x".repeat(65)] {
            assert!(valid_id(bad).is_err(), "{bad:?}");
        }
        assert!(valid_id("web-1.a_b").is_ok());
    }

    #[test]
    fn atomic_write_replaces_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
    }
}
