---
title: "1 · Spec e erros"
slug: mc-spec-e-erros
summary: A runtime-spec como tipos, validação que recusa em vez de ignorar, e um enum de erros que decide o código de saída.
part: 4
order: 10
time: 15 min de leitura
---

# 1 · Spec e erros

Começamos por onde o input entra. Antes de tocar no kernel, o `mc` tem de responder a duas perguntas: **o que me pedem?** e **posso cumprir?**

## A spec como tipos

O `config.json` da runtime-spec tem dezenas de campos; o `minicontainer` modela **um subconjunto**, com `serde`:

{{file:minicontainer/src/spec.rs#spec-structs}}

Decisões de design que se repetem em todo o motor:

- **`#[serde(default)]` nos opcionais** — um `config.json` mínimo tem de bastar.
- **Sem `deny_unknown_fields`** — a spec OCI tem muitos campos que ignoramos legitimamente (`user`, `capabilities`, `annotations`…), e um documento gerado pelo `runc spec` tem de carregar. É um compromisso consciente: o teste `parses_a_real_runc_style_document` fixa-o.
- **`seccomp: Option<Value>` só existe para o poder recusar.** Deserializamos o campo *para saber se veio*, e depois recusamos.

## Recusar, não ignorar

Esta é **a** regra do capítulo: *um campo que o cliente escreve e o sistema ignora é pior do que um campo que não existe.* Se um bundle pede seccomp e o runtime o ignora em silêncio, o operador julga o container protegido e não está. Por isso o `validate` é uma lista de recusas com **nome**:

{{file:minicontainer/src/spec.rs#spec-validate}}

Repare no que acontece com os namespaces: um bundle que peça **menos** isolamento do que o runtime dá (por exemplo, sem `network`) também é recusado — porque este runtime *sempre* isola, e um bundle que julgue estar na rede do host estaria enganado.

Os testes fixam cada recusa:

```rust
#[test]
fn refuses_what_it_cannot_honour() {
    let mut s = base();
    s.linux.seccomp = Some(serde_json::json!({"defaultAction": "SCMP_ACT_ALLOW"}));
    assert!(matches!(s.validate(), Err(Error::Unsupported(_))));
    // … terminal: true, e namespaces com `path` (juntar-se a um existente)
}
```

E o efeito, visto pelo utilizador:

{{out:06-seccomp-recusado}}

O código de saída `2` (não `1`) — porque «o pedido é inválido» é uma classe diferente de «rebentou».

## Erros: um enum, uma tabela

{{file:minicontainer/src/error.rs#error}}

Três coisas para reparar:

1. **`#[from] nix::Error`** — o operador `?` converte automaticamente erros de chamadas de sistema. O `thiserror` gera o `From`.
2. **`Error::io(contexto, source)`** e o *trait* de extensão `.ctx(|| …)` — um `io::Error` sozinho diz *«No such file or directory»* sem dizer **qual** ficheiro. Acrescentar o contexto no sítio onde se sabe (`reading /path/config.json`) custa uma linha e poupa uma hora de depuração.
3. **`exit_code()` num só sítio.** Os códigos que vês nas saídas dos capítulos seguintes — `4` para «não existe», `5` para «já existe» ou «estado errado», `2` para spec inválida — vêm **todos** desta função:

| Situação | Erro | Saída |
|---|---|---|
| `mc state naoexiste` | `NotFound` | **4** |
| `mc create` com um id já usado | `Conflict` | **5** |
| `mc delete` de um container vivo | `WrongState` | **5** |
| bundle com seccomp | `Unsupported` | **2** |
| tudo o resto | — | **1** |
| o **workload** faz `exit 7` | — | **7** (propagado) |

!!! warning "Armadilha: o código do *workload* não é o do runtime"
    `mc run` devolve o código de saída **do processo do container** (7, 127, 137…), não uma das classes acima. É a regressão mais fácil de introduzir: se o `run` passasse por `Error::exit_code`, um `exit 4` do utilizador confundir-se-ia com «container não existe». O teste `propagates_the_exit_code_of_the_workload` guarda-o.

## Validar identificadores

O id do container entra num caminho de disco (`<estado>/<id>/`) e num nome de cgroup (`mc-<id>`). Lista branca, não «limpeza»:

{{file:minicontainer/src/fsutil.rs#valid-id}}

O teste percorre os casos hostis — `""`, `".."`, `"-x"`, `"a/b"`, `"a b"`, 65 caracteres.

## Verifica

```bash
cargo test -p minicontainer spec::        # 4 testes
cargo test -p minicontainer fsutil::      # 4 testes
```

Próximo: com o input validado, a primeira coisa que o runtime faz com uma imagem — [desempacotá-la](mc-imagem-oci.html).
