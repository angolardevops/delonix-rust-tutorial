//! Erros tipados da biblioteca.
//!
//! Boa prática: uma biblioteca devolve um `enum` fechado (`thiserror`), não `anyhow`.
//! Quem chama (o `main`) decide como o erro vira código de saída — ver [`Error::exit_code`].

use std::io;
use std::path::PathBuf;

// region: error
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },

    #[error("invalid JSON in {path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },

    #[error("system call failed: {0}")]
    Sys(#[from] nix::Error),

    /// O bundle/config.json está mal formado ou incoerente.
    #[error("invalid bundle: {0}")]
    Spec(String),

    /// O bundle pede algo que este runtime NÃO implementa. Recusa-se — nunca se ignora.
    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("digest mismatch for {what}: expected {expected}, got {actual}")]
    Digest { what: String, expected: String, actual: String },

    #[error("unsafe path refused: {0}")]
    UnsafePath(PathBuf),

    #[error("no such container: {0}")]
    NotFound(String),

    #[error("container already exists: {0}")]
    Conflict(String),

    #[error("container {id} is {status}, expected {expected}")]
    WrongState { id: String, status: String, expected: &'static str },

    #[error("cgroup: {0}")]
    Cgroup(String),

    #[error("container setup failed: {0}")]
    Setup(String),
}

impl Error {
    /// Constrói um `Error::Io` com contexto (o caminho ou a acção que falhou).
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io { context: context.into(), source }
    }

    /// Classe de saída, ao estilo do delonix: «não existe» (4) e «conflito» (5)
    /// distinguem-se de «rebentou» (1) sem ninguém ter de ler a mensagem.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::NotFound(_) => 4,
            Self::Conflict(_) | Self::WrongState { .. } => 5,
            Self::Spec(_) | Self::Unsupported(_) => 2,
            _ => 1,
        }
    }
}

/// Extensão para anexar contexto a um `io::Result` sem repetir `map_err` em todo o lado.
pub trait IoContext<T> {
    fn ctx(self, context: impl FnOnce() -> String) -> Result<T>;
}

impl<T> IoContext<T> for std::result::Result<T, io::Error> {
    fn ctx(self, context: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|e| Error::io(context(), e))
    }
}
// endregion
