//! **OCI image-spec**: desempacota um *image layout* para um *bundle* de runtime.
//!
//! image layout = `oci-layout` + `index.json` + `blobs/sha256/<hex>`.
//! O caminho: index → manifest → (config, layers). **Todo o blob é verificado contra o
//! digest que o referencia antes de ser usado** — é o que faz de um digest um *pin* a sério
//! (o delonix já apanhou um registo onde o digest era decorativo).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{Error, IoContext, Result};
use crate::fsutil::write_atomic;
use crate::spec::Spec;

#[derive(Debug, Deserialize)]
struct Index {
    manifests: Vec<Descriptor>,
}

#[derive(Debug, Deserialize)]
struct Descriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    config: Descriptor,
    layers: Vec<Descriptor>,
}

#[derive(Debug, Deserialize)]
struct ImageConfig {
    #[serde(default)]
    config: RuntimeConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RuntimeConfig {
    #[serde(default)]
    entrypoint: Vec<String>,
    #[serde(default)]
    cmd: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    working_dir: String,
}

// region: blob-path
/// `sha256:<64 hex>` → caminho do blob. A validação do formato NÃO é cosmética:
/// um `digest` como `sha256:../../etc/passwd` sairia do layout.
fn blob_path(layout: &Path, digest: &str) -> Result<(PathBuf, String)> {
    let hex = digest
        .strip_prefix("sha256:")
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| Error::Spec(format!("unsupported digest {digest:?}")))?;
    Ok((layout.join("blobs/sha256").join(hex), hex.to_ascii_lowercase()))
}
// endregion

fn sha256_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).ctx(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ctx(|| format!("hashing {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

// region: verified
/// Devolve o caminho do blob **já verificado**.
fn verified_blob(layout: &Path, digest: &str) -> Result<PathBuf> {
    let (path, expected) = blob_path(layout, digest)?;
    let actual = sha256_file(&path)?;
    if actual != expected {
        return Err(Error::Digest { what: digest.into(), expected, actual });
    }
    Ok(path)
}
// endregion

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).ctx(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Json { path: path.into(), source })
}

// region: unpack
/// Desempacota `layout` em `bundle/rootfs` e gera `bundle/config.json`.
pub fn unpack(layout: &Path, bundle: &Path) -> Result<()> {
    let index: Index = read_json(&layout.join("index.json"))?;
    let entry = index
        .manifests
        .iter()
        .find(|d| d.media_type == "application/vnd.oci.image.manifest.v1+json")
        .ok_or_else(|| Error::Unsupported("index.json without an OCI image manifest".into()))?;

    let manifest: Manifest = read_json(&verified_blob(layout, &entry.digest)?)?;
    let config: ImageConfig = read_json(&verified_blob(layout, &manifest.config.digest)?)?;

    // Verifica TODAS as layers antes de escrever qualquer byte no bundle.
    let layers: Vec<(PathBuf, &str)> = manifest
        .layers
        .iter()
        .map(|d| Ok((verified_blob(layout, &d.digest)?, d.media_type.as_str())))
        .collect::<Result<_>>()?;

    let rootfs = bundle.join("rootfs");
    fs::create_dir_all(&rootfs).ctx(|| format!("creating {}", rootfs.display()))?;
    for (path, media_type) in &layers {
        apply_layer(path, media_type, &rootfs)?;
    }

    let cfg = &config.config;
    let mut args = cfg.entrypoint.clone();
    args.extend(cfg.cmd.iter().cloned());
    if args.is_empty() {
        return Err(Error::Spec("image has neither Entrypoint nor Cmd".into()));
    }
    let mut spec = Spec::new_default(args);
    if !cfg.env.is_empty() {
        spec.process.env = cfg.env.clone();
    }
    if !cfg.working_dir.is_empty() {
        spec.process.cwd = cfg.working_dir.clone();
    }
    let json = serde_json::to_vec_pretty(&spec)
        .map_err(|source| Error::Json { path: bundle.join("config.json"), source })?;
    write_atomic(&bundle.join("config.json"), &json)
}
// endregion

// region: layer
fn apply_layer(blob: &Path, media_type: &str, rootfs: &Path) -> Result<()> {
    let file = fs::File::open(blob).ctx(|| format!("opening {}", blob.display()))?;
    let reader: Box<dyn Read> = match media_type {
        "application/vnd.oci.image.layer.v1.tar+gzip" => Box::new(flate2::read::GzDecoder::new(file)),
        "application/vnd.oci.image.layer.v1.tar" => Box::new(file),
        other => return Err(Error::Unsupported(format!("layer media type {other}"))),
    };
    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_ownerships(false); // sem root não há chown; e não devia haver.
    archive.set_overwrite(true);

    for entry in archive.entries().ctx(|| format!("reading {}", blob.display()))? {
        let mut entry = entry.ctx(|| "reading tar entry".to_string())?;
        let path = entry.path().ctx(|| "tar entry path".to_string())?.into_owned();
        let kind = entry.header().entry_type();

        // Whiteouts: `.wh.<nome>` apaga o que as layers de baixo puseram; `.wh..wh..opq` esvazia o dir.
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && let Some(target) = name.strip_prefix(".wh.")
        {
            let parent =
                crate::fsutil::resolve_in_root(rootfs, path.parent().unwrap_or_else(|| Path::new("")), false);
            if let Ok(parent) = parent {
                if target == ".wh..opq" {
                    empty_dir(&parent)?;
                } else {
                    remove_any(&parent.join(target))?;
                }
            }
            continue;
        }
        // Sem root não se criam device nodes — e um device dentro de uma imagem é suspeito.
        if kind.is_block_special() || kind.is_character_special() {
            eprintln!("warning: skipping device node {}", path.display());
            continue;
        }
        // `unpack_in` recusa `..` e symlinks que escapem do destino.
        if !entry.unpack_in(rootfs).ctx(|| format!("unpacking {}", path.display()))? {
            return Err(Error::UnsafePath(path));
        }
    }
    make_dirs_writable(rootfs)
}
// endregion

fn empty_dir(dir: &Path) -> Result<()> {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            remove_any(&e.path())?;
        }
    }
    Ok(())
}

fn remove_any(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path).ctx(|| format!("removing {}", path.display())),
        Ok(_) => fs::remove_file(path).ctx(|| format!("removing {}", path.display())),
        Err(_) => Ok(()),
    }
}

/// Uma layer pode trazer directórios 0555; a layer seguinte precisa de lá escrever.
fn make_dirs_writable(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let Ok(rd) = fs::read_dir(dir) else {
        return Ok(());
    };
    for e in rd.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() && !meta.file_type().is_symlink() {
            let mut perm = meta.permissions();
            perm.set_mode(perm.mode() | 0o700);
            let _ = fs::set_permissions(e.path(), perm);
            make_dirs_writable(&e.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_format_is_validated_before_touching_disk() {
        let l = Path::new("/nonexistent");
        for bad in ["sha256:../../etc/passwd", "sha256:abc", "md5:00", "sha256:"] {
            assert!(blob_path(l, bad).is_err(), "{bad}");
        }
        assert!(blob_path(l, &format!("sha256:{}", "a".repeat(64))).is_ok());
    }

    #[test]
    fn a_tampered_blob_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = dir.path().join("blobs/sha256");
        fs::create_dir_all(&blobs).unwrap();
        let claimed = "0".repeat(64);
        fs::write(blobs.join(&claimed), b"not what the digest says").unwrap();
        let err = verified_blob(dir.path(), &format!("sha256:{claimed}")).unwrap_err();
        assert!(matches!(err, Error::Digest { .. }), "{err}");
    }
}
