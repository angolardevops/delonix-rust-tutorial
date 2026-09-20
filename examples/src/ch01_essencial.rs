//! Capítulo 1 — Rust essencial, com exemplos de motor de containers.

// ─── Ownership e empréstimos ────────────────────────────────────────────────────────────────

/// `&str` empresta: quem chama continua dono da `String`.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `String` por valor MOVE: depois da chamada o chamador já não a pode usar.
pub fn into_label(name: String) -> String {
    format!("delonix.io/name={name}")
}

// ─── Enums que carregam dados + match exaustivo ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Created,
    Running { pid: i32 },
    Stopped { exit_code: i32 },
}

impl Status {
    /// `match` **exaustivo**: se acrescentares uma variante, isto deixa de compilar até
    /// decidires o que fazer com ela — é o que substitui os `default:` silenciosos de outras linguagens.
    pub fn describe(&self) -> String {
        match self {
            Self::Created => "created".to_owned(),
            Self::Running { pid } => format!("running (pid {pid})"),
            Self::Stopped { exit_code: 0 } => "exited cleanly".to_owned(),
            Self::Stopped { exit_code } => format!("exited with {exit_code}"),
        }
    }
}

// ─── Result e o operador `?` ────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    NotANumber(String),
    Negative(i64),
}

/// Converte `"64M"`, `"2G"`, `"512"` em bytes. Cada `?` devolve o erro ao chamador.
pub fn parse_size(s: &str) -> Result<u64, ParseError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(ParseError::Empty);
    }
    let (digits, mult) = match s.chars().last() {
        Some('K') => (&s[..s.len() - 1], 1u64 << 10),
        Some('M') => (&s[..s.len() - 1], 1 << 20),
        Some('G') => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    let n: i64 = digits.parse().map_err(|_| ParseError::NotANumber(s.to_owned()))?;
    let n = u64::try_from(n).map_err(|_| ParseError::Negative(n))?;
    Ok(n.saturating_mul(mult)) // saturating: input hostil não pode dar overflow silencioso
}

// ─── Traits: o padrão «backend» ─────────────────────────────────────────────────────────────

/// Mesma forma que o `VmBackend` do delonix: o motor fala com a *interface*, e cada
/// hypervisor é uma implementação — nunca um `if provider == "libvirt"` espalhado.
pub trait Backend {
    fn id(&self) -> &'static str;
    fn boot(&self, name: &str) -> Result<u32, String>;
}

#[derive(Debug)]
pub struct Local;
#[derive(Debug)]
pub struct Remote {
    pub endpoint: String,
}

impl Backend for Local {
    fn id(&self) -> &'static str {
        "local"
    }
    fn boot(&self, _name: &str) -> Result<u32, String> {
        Ok(1)
    }
}

impl Backend for Remote {
    fn id(&self) -> &'static str {
        "remote"
    }
    fn boot(&self, name: &str) -> Result<u32, String> {
        if self.endpoint.is_empty() {
            return Err(format!("cannot boot {name}: no endpoint"));
        }
        Ok(2)
    }
}

/// Genérico sobre o trait (despacho estático, sem custo) …
pub fn boot_static<B: Backend>(b: &B, name: &str) -> Result<u32, String> {
    b.boot(name)
}

/// … ou por `dyn` quando o backend só se conhece em runtime (um registo de backends).
pub fn boot_all(backends: &[Box<dyn Backend>], name: &str) -> Vec<(&'static str, Result<u32, String>)> {
    backends.iter().map(|b| (b.id(), b.boot(name))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowing_keeps_ownership() {
        let name = String::from("web-1");
        assert!(is_valid_name(&name));
        assert_eq!(name, "web-1"); // ainda é nosso
        assert_eq!(into_label(name), "delonix.io/name=web-1"); // `name` movido aqui
    }

    #[test]
    fn match_describes_every_state() {
        assert_eq!(Status::Created.describe(), "created");
        assert_eq!(Status::Running { pid: 7 }.describe(), "running (pid 7)");
        assert_eq!(Status::Stopped { exit_code: 0 }.describe(), "exited cleanly");
        assert_eq!(Status::Stopped { exit_code: 137 }.describe(), "exited with 137");
    }

    #[test]
    fn parse_size_handles_units_and_hostile_input() {
        assert_eq!(parse_size("64M"), Ok(64 << 20));
        assert_eq!(parse_size("2G"), Ok(2 << 30));
        assert_eq!(parse_size(""), Err(ParseError::Empty));
        assert_eq!(parse_size("x"), Err(ParseError::NotANumber("x".into())));
        assert_eq!(parse_size("-1"), Err(ParseError::Negative(-1)));
        // 99999999999T não existe, mas 9999999999999G satura em vez de dar a volta
        assert_eq!(parse_size("9999999999999G"), Ok(u64::MAX));
    }

    #[test]
    fn traits_allow_static_and_dynamic_dispatch() {
        assert_eq!(boot_static(&Local, "a"), Ok(1));
        let all: Vec<Box<dyn Backend>> = vec![Box::new(Local), Box::new(Remote { endpoint: String::new() })];
        let out = boot_all(&all, "vm");
        assert_eq!(out[0], ("local", Ok(1)));
        assert!(out[1].1.is_err());
    }
}
