//! Capítulo 6 — Rust para sistemas distribuídos: os padrões que o delonix usa num só nó
//! e que escalam para uma frota.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

// ─── 1. Reconciliação de 3 vias — uma função PURA ───────────────────────────────────────────
//
// `plan` recebe os três lados já lidos e devolve o que mudar. Nunca toca no sistema.
// É por isso que os casos difíceis se testam como dados, em microssegundos.

// region: plan
pub type Fields = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Create,
    /// Campos a alterar, chave → novo valor.
    Update(Fields),
    /// Removido do manifesto e foi criado por nós: apagar.
    Delete,
    NoOp,
}

/// - `desired`: o manifesto de agora.
/// - `actual`: o que existe na máquina (`None` = não existe).
/// - `last_applied`: o que aplicámos da última vez (guardado no próprio recurso).
pub fn plan(desired: &Fields, actual: Option<&Fields>, last_applied: Option<&Fields>) -> Action {
    let Some(actual) = actual else {
        return Action::Create;
    };
    let mut changes = Fields::new();
    for (k, want) in desired {
        if actual.get(k) != Some(want) {
            changes.insert(k.clone(), want.clone());
        }
    }
    // Campo que já foi NOSSO (estava em last_applied) e saiu do manifesto → reverter.
    // Campo que nunca foi nosso (alguém pôs à mão) → não tocar. É isto que o 3.º lado distingue.
    if let Some(last) = last_applied {
        for k in last.keys().filter(|k| !desired.contains_key(*k)) {
            if actual.contains_key(k) {
                changes.insert(k.clone(), String::new());
            }
        }
    }
    if changes.is_empty() { Action::NoOp } else { Action::Update(changes) }
}

/// Recurso removido do manifesto: só se apaga o que é NOSSO (tem dono = esta stack).
pub fn should_prune(owner: Option<&str>, stack: &str) -> bool {
    owner == Some(stack)
}
// endregion

// ─── 2. Idempotência: aplicar duas vezes = aplicar uma ──────────────────────────────────────

// region: apply
#[derive(Debug, Default)]
pub struct Machine {
    pub resources: BTreeMap<String, Fields>,
    pub writes: usize, // quantas vezes tocámos realmente na máquina
}

/// `apply` converge. Chamá-lo N vezes deixa o mesmo estado e — a prova — **zero escritas**
/// a partir da segunda.
pub fn apply(m: &mut Machine, name: &str, desired: &Fields) {
    match plan(desired, m.resources.get(name), None) {
        Action::Create => {
            m.resources.insert(name.to_owned(), desired.clone());
            m.writes += 1;
        }
        Action::Update(ch) => {
            if let Some(r) = m.resources.get_mut(name) {
                r.extend(ch);
            }
            m.writes += 1;
        }
        Action::Delete | Action::NoOp => {}
    }
}
// endregion

// ─── 3. Retentativas com backoff exponencial e tecto ────────────────────────────────────────

// region: backoff
/// 1s, 2s, 4s, 8s … até ao tecto. Sem tecto, uma falha longa dorme horas; sem *jitter*
/// (não incluído aqui por ser determinístico nos testes) uma frota inteira retenta em uníssono.
pub fn backoff(attempt: u32, base: Duration, cap: Duration) -> Duration {
    let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
    base.saturating_mul(factor).min(cap)
}

/// Só se retenta o que faz sentido retentar: um 404 nunca melhora à 5.ª tentativa.
pub fn is_retryable(http_status: u16) -> bool {
    matches!(http_status, 408 | 429 | 500 | 502 | 503 | 504)
}
// endregion

// ─── 4. Retomar um download: o servidor responde a OUTRA pergunta? ──────────────────────────

// region: range
/// Valida um `Content-Range: bytes <inicio>-<fim>/<total>` contra o offset que pedimos.
/// Um 206 noutro offset «responde a outra pergunta»: colar duplicaria o prefixo e a corrupção
/// só apareceria no digest, depois de pagar o download inteiro.
pub fn parse_content_range(h: &str, expected_start: u64) -> Option<u64> {
    let rest = h.strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    (start.parse::<u64>().ok()? == expected_start).then(|| total.parse().ok())?
}
// endregion

// ─── 5. Estado em disco sem corridas: escrita atómica + flock ───────────────────────────────

// region: atomic
/// Escrever num ficheiro temporário e fazer `rename`: um leitor vê o antigo inteiro
/// ou o novo inteiro, nunca metade. A ORDEM importa: `fsync` do conteúdo antes do `rename`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("f"),
        std::process::id()
    ));
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}
// endregion

// ─── 6. Erros com classe: o chamador decide sem parsear mensagens ───────────────────────────

// region: exit
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("timeout after {0:?}")]
    Timeout(Duration),
    #[error("{0}")]
    Other(String),
}

/// Uma tabela pequena, num só sítio. As mensagens são traduzidas (pt/en) e mudam;
/// o código de saída é contrato — um script que faça `grep 'no such'` parte-se com `--lang=pt`.
pub fn exit_code(e: &EngineError) -> i32 {
    match e {
        EngineError::NotFound(_) => 4,
        EngineError::Conflict(_) => 5,
        EngineError::Timeout(_) => 124,
        EngineError::Other(_) => 1,
    }
}
// endregion

#[cfg(test)]
mod tests {
    use super::*;

    fn f(pairs: &[(&str, &str)]) -> Fields {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn three_way_diff_separates_our_removals_from_foreign_edits() {
        let desired = f(&[("image", "nginx:2")]);
        let actual = f(&[("image", "nginx:1"), ("memory", "64M"), ("debug", "on")]);
        let last = f(&[("image", "nginx:1"), ("memory", "64M")]);
        let Action::Update(ch) = plan(&desired, Some(&actual), Some(&last)) else {
            panic!("expected update")
        };
        assert_eq!(ch.get("image").map(String::as_str), Some("nginx:2")); // mudou no manifesto
        assert_eq!(ch.get("memory").map(String::as_str), Some("")); // era nosso e saiu → reverter
        assert!(!ch.contains_key("debug")); // posto à mão por alguém → NÃO tocar
    }

    #[test]
    fn plan_is_create_update_or_noop() {
        let d = f(&[("a", "1")]);
        assert_eq!(plan(&d, None, None), Action::Create);
        assert_eq!(plan(&d, Some(&d), Some(&d)), Action::NoOp);
    }

    #[test]
    fn only_our_own_resources_are_pruned() {
        assert!(should_prune(Some("shop"), "shop"));
        assert!(!should_prune(Some("other"), "shop"));
        assert!(!should_prune(None, "shop")); // criado à mão: sem dono, sobrevive
    }

    #[test]
    fn apply_is_idempotent() {
        let mut m = Machine::default();
        let d = f(&[("image", "nginx")]);
        apply(&mut m, "web", &d);
        assert_eq!(m.writes, 1);
        apply(&mut m, "web", &d);
        apply(&mut m, "web", &d);
        assert_eq!(m.writes, 1, "reaplicar não pode escrever");
    }

    #[test]
    fn backoff_grows_and_is_capped_without_overflow() {
        let (b, c) = (Duration::from_secs(1), Duration::from_secs(30));
        let seq: Vec<u64> = (0..7).map(|i| backoff(i, b, c).as_secs()).collect();
        assert_eq!(seq, [1, 2, 4, 8, 16, 30, 30]);
        assert_eq!(backoff(500, b, c), c); // shift enorme não dá pânico
    }

    #[test]
    fn retries_only_transient_failures() {
        assert!(is_retryable(503) && is_retryable(429));
        assert!(!is_retryable(404) && !is_retryable(401));
    }

    #[test]
    fn resume_rejects_a_206_for_the_wrong_offset() {
        assert_eq!(parse_content_range("bytes 3000-5999/6000", 3000), Some(6000));
        assert_eq!(parse_content_range("bytes 0-5999/6000", 3000), None); // outra pergunta
        assert_eq!(parse_content_range("garbage", 0), None);
    }

    #[test]
    fn atomic_write_never_exposes_a_half_file() {
        let dir = std::env::temp_dir().join(format!("ex-atomic-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("state.json");
        write_atomic(&p, b"{\"v\":1}").unwrap();
        write_atomic(&p, b"{\"v\":2}").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"{\"v\":2}");
        assert!(fs::read_dir(&dir).unwrap().count() == 1, "sem temporários órfãos");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn exit_codes_are_a_stable_contract() {
        assert_eq!(exit_code(&EngineError::NotFound("x".into())), 4);
        assert_eq!(exit_code(&EngineError::Conflict("x".into())), 5);
        assert_eq!(exit_code(&EngineError::Timeout(Duration::from_secs(1))), 124);
        assert_eq!(exit_code(&EngineError::Other("x".into())), 1);
    }
}
