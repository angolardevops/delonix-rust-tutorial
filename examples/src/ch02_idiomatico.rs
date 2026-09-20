//! Capítulo 2 — Rust idiomático: tipos que tornam os estados inválidos impossíveis.

use std::marker::PhantomData;

// ─── Newtype com validação na fronteira ─────────────────────────────────────────────────────

// region: newtype
/// Um id de container **validado uma vez**, à entrada. Todo o código a jusante recebe
/// `ContainerId` e não precisa de re-validar — e não há forma de construir um inválido.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerId(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("container id must not be empty")]
    Empty,
    #[error("container id too long ({0} > 64)")]
    TooLong(usize),
    #[error("invalid character {0:?} in container id")]
    BadChar(char),
    #[error("container id must not start with '-' or '.'")]
    BadStart,
}

impl TryFrom<&str> for ContainerId {
    type Error = IdError;

    fn try_from(s: &str) -> Result<Self, IdError> {
        if s.is_empty() {
            return Err(IdError::Empty);
        }
        if s.len() > 64 {
            return Err(IdError::TooLong(s.len()));
        }
        if s.starts_with(['-', '.']) {
            return Err(IdError::BadStart);
        }
        if let Some(c) = s.chars().find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))) {
            return Err(IdError::BadChar(c));
        }
        Ok(Self(s.to_owned()))
    }
}

impl ContainerId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
// endregion

// ─── Typestate: transições ilegais não compilam ─────────────────────────────────────────────

// region: typestate
#[derive(Debug)]
pub struct Created;
#[derive(Debug)]
pub struct Running;
#[derive(Debug)]
pub struct Stopped;

/// O parâmetro `S` é o estado. Cada método só existe no estado onde faz sentido, e
/// **consome** `self`: a fase antiga deixa de existir.
///
/// ```
/// use examples::ch02_idiomatico::{Container, ContainerId};
/// let id = ContainerId::try_from("web").unwrap();
/// let c = Container::new(id).start(4242).stop(0);
/// assert_eq!(c.exit_code(), 0);
/// ```
///
/// Isto **não compila** (`stop` só existe em `Container<Running>`):
///
/// ```compile_fail
/// use examples::ch02_idiomatico::{Container, ContainerId};
/// let c = Container::new(ContainerId::try_from("web").unwrap());
/// c.stop(0);
/// ```
///
/// Nem isto (`c` foi consumido pela primeira transição):
///
/// ```compile_fail
/// use examples::ch02_idiomatico::{Container, ContainerId};
/// let c = Container::new(ContainerId::try_from("web").unwrap());
/// let _r = c.start(1);
/// c.start(2);
/// ```
#[derive(Debug)]
pub struct Container<S> {
    id: ContainerId,
    pid: i32,
    exit_code: i32,
    _state: PhantomData<S>,
}

impl Container<Created> {
    pub fn new(id: ContainerId) -> Self {
        Self { id, pid: 0, exit_code: 0, _state: PhantomData }
    }
    pub fn start(self, pid: i32) -> Container<Running> {
        Container { id: self.id, pid, exit_code: 0, _state: PhantomData }
    }
}

impl Container<Running> {
    pub fn pid(&self) -> i32 {
        self.pid
    }
    pub fn stop(self, exit_code: i32) -> Container<Stopped> {
        Container { id: self.id, pid: self.pid, exit_code, _state: PhantomData }
    }
}

impl Container<Stopped> {
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }
    pub fn restart(self) -> Container<Created> {
        Container::new(self.id)
    }
}

impl<S> Container<S> {
    pub fn id(&self) -> &ContainerId {
        &self.id
    }
}
// endregion

// ─── Builder: muitos parâmetros opcionais, um único ponto de validação ──────────────────────

// region: builder
#[derive(Debug, PartialEq, Eq)]
pub struct RunSpec {
    pub image: String,
    pub memory: Option<u64>,
    pub env: Vec<String>,
    pub read_only: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpecError {
    #[error("an image is required")]
    NoImage,
    #[error("env entry {0:?} is not KEY=VALUE")]
    BadEnv(String),
}

#[derive(Debug, Default)]
pub struct RunSpecBuilder {
    image: Option<String>,
    memory: Option<u64>,
    env: Vec<String>,
    read_only: bool,
}

impl RunSpecBuilder {
    pub fn image(mut self, i: impl Into<String>) -> Self {
        self.image = Some(i.into());
        self
    }
    pub fn memory(mut self, bytes: u64) -> Self {
        self.memory = Some(bytes);
        self
    }
    pub fn env(mut self, kv: impl Into<String>) -> Self {
        self.env.push(kv.into());
        self
    }
    pub fn read_only(mut self, yes: bool) -> Self {
        self.read_only = yes;
        self
    }
    /// Toda a validação vive aqui — `RunSpec` só existe válido.
    pub fn build(self) -> Result<RunSpec, SpecError> {
        let image = self.image.ok_or(SpecError::NoImage)?;
        if let Some(bad) = self.env.iter().find(|e| !e.contains('=')) {
            return Err(SpecError::BadEnv(bad.clone()));
        }
        Ok(RunSpec { image, memory: self.memory, env: self.env, read_only: self.read_only })
    }
}
// endregion

// ─── Iteradores em vez de laços com estado ──────────────────────────────────────────────────

/// Soma a memória pedida dos containers que vão correr, ignorando os sem limite.
pub fn total_memory(specs: &[RunSpec]) -> u64 {
    specs.iter().filter_map(|s| s.memory).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_validated_once_at_the_boundary() {
        assert!(ContainerId::try_from("web-1.a_b").is_ok());
        assert_eq!(ContainerId::try_from(""), Err(IdError::Empty));
        assert_eq!(ContainerId::try_from("../etc"), Err(IdError::BadStart));
        assert_eq!(ContainerId::try_from("a/b"), Err(IdError::BadChar('/')));
        assert_eq!(ContainerId::try_from(&*"x".repeat(65)), Err(IdError::TooLong(65)));
    }

    #[test]
    fn lifecycle_goes_forward_and_can_restart() {
        let c = Container::new(ContainerId::try_from("c1").unwrap());
        let r = c.start(10);
        assert_eq!(r.pid(), 10);
        let s = r.stop(137);
        assert_eq!(s.exit_code(), 137);
        let again = s.restart();
        assert_eq!(again.id().as_str(), "c1");
    }

    #[test]
    fn builder_validates_in_one_place() {
        assert_eq!(RunSpecBuilder::default().build(), Err(SpecError::NoImage));
        assert_eq!(
            RunSpecBuilder::default().image("alpine").env("NOVALUE").build(),
            Err(SpecError::BadEnv("NOVALUE".into()))
        );
        let ok = RunSpecBuilder::default()
            .image("alpine")
            .memory(1 << 20)
            .env("A=b")
            .read_only(true)
            .build()
            .unwrap();
        assert_eq!(total_memory(&[ok]), 1 << 20);
    }
}
