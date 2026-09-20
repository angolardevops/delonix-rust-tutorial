---
title: Rust idiomático
slug: rust-idiomatico
summary: Newtypes, typestate, builders, erros tipados e unsafe disciplinado — as técnicas que tornam estados inválidos impossíveis.
part: 1
order: 2
time: 30 min de leitura
---

# Rust idiomático

Saber a sintaxe não chega: o que separa código Rust de sistemas *bom* é **fazer o compilador trabalhar para ti**. A ideia central deste capítulo, e de todo o delonix: *parse, don't validate* — valida uma vez, na fronteira, e a partir daí o tipo **prova** que o valor é válido.

## Newtype: validar uma vez, à entrada

Um id de container entra em caminhos de disco, nomes de cgroup e argumentos de `ssh`. Se for uma `String` solta, todo o código a jusante tem de desconfiar dele — e basta um esquecer-se para haver um *path traversal*. Com um *newtype*, a validação acontece **uma vez** e o tipo carrega a garantia:

{{file:examples/src/ch02_idiomatico.rs#newtype}}

`ContainerId` tem o campo privado e só se constrói por `TryFrom`. Não há forma de obter um id inválido — logo nenhuma função que o receba precisa de o re-validar.

!!! delonix "Um bug real desta classe"
    O delonix já teve um *path traversal* por nome de VM (`metadata.name: "../../.ssh/authorized_keys"` escrevia fora do directório de estado) e outro por `--name registry.npmjs.org` a sequestrar o DNS interno. A correcção nos dois casos foi a mesma: validar **na fronteira do motor** (`valid_vm_name`, `valid_container_name`), com lista branca de caracteres — nunca «limpar» o input.

## Typestate: transições ilegais que não compilam

Em vez de um campo `status` verificado em runtime, codifica o estado **no tipo**. Cada transição **consome** `self`, por isso a fase antiga deixa de existir:

{{file:examples/src/ch02_idiomatico.rs#typestate}}

Os `compile_fail` nos comentários de documentação são **testes**: `cargo test --doc` confirma que `c.stop(0)` num container ainda `Created` *não compila*. É a mesma técnica que o motor usa no ciclo de vida do container:

{{snippet:typestate}}

!!! tip "Quando usar — e quando não"
    Typestate brilha quando as fases são **poucas, lineares e conhecidas em compilação** (ciclo de vida de um objecto que vive num só processo). Não o uses para estado que vem do disco ou da rede (aí o estado só se conhece em *runtime*: usa um `enum` e `match` exaustivo). O delonix usa `enum Status` persistido **e** typestate no código que orquestra a transição.

## Builder: muitos opcionais, um único ponto de validação

{{file:examples/src/ch02_idiomatico.rs#builder}}

`RunSpec` só existe **válido**: toda a validação vive em `build()`. Chamar `.env("SEM_IGUAL")` só falha quando construíres — com uma mensagem que nomeia o valor, não com um pânico três camadas abaixo.

## Erros tipados, com classe

Duas escolas, e ambas têm o seu sítio:

| | `thiserror` (bibliotecas) | `anyhow` (aplicações) |
|---|---|---|
| Tipo de erro | `enum` fechado, cada variante documentada | `anyhow::Error` opaco |
| O chamador pode… | fazer `match` e decidir | só imprimir/propagar |
| Usa em | crates de motor (`delonix-*`) | o `main` de uma ferramenta pequena |

O motor vai um passo além: **o erro tem uma classe** que vira código de saída, num único sítio.

{{snippet:exit-codes}}

Isto não é cosmético. As mensagens são traduzidas (`--l18n=pt`) e mudam; um script que faça `grep 'no such'` funciona na máquina onde foi escrito e **deixa de classificar** num nó em português. O código de saída é contrato: `4` = «não existe», `5` = «conflito», `1` = «rebentou». É o que um reconciliador precisa para decidir *cria* ou *pára*. Vais implementar o mesmo no `minicontainer` ([erros](mc-spec-e-erros.html)).

!!! warning "Armadilha: um `match` com `_ =>`"
    O `match` da tabela acima é **exaustivo de propósito**. Se lhe pusesses um `_ =>`, uma variante nova de `Error` seria arquivada em «genérico» sem ninguém decidir. Sem o `_`, o compilador pára e obriga a escolher.

## `unsafe`: pequeno, comentado, isolado

Um runtime **tem** de usar `unsafe` (`fork`, `prctl`, `ioctl`). A disciplina:

1. **Mínimo** — o bloco mais pequeno possível, e nunca em torno de lógica.
2. **`// SAFETY:`** — um comentário que diz *porque* as pré-condições se verificam ali.
3. **Isolado** — atrás de uma função segura, para o resto do código não ver `unsafe`.

```rust
// SAFETY: o processo é single-thread neste ponto (nenhuma thread foi lançada), logo
// `fork` é seguro; o filho só chama funções async-signal-safe até ao `exec`/`_exit`.
match unsafe { fork() }? { /* ... */ }
```

Este comentário é do `minicontainer`, e a razão é real: `fork()` num processo **multi-thread** só deixa viva a thread que chamou, e se outra segurava o *lock* do `malloc`, o filho bloqueia para sempre. O delonix pagou isto: o servidor da API Docker era multi-thread e o `clone()` do arranque de containers **bloqueava sob pedidos concorrentes**; a correcção foi arrancar por *re-exec* de um processo novo.

!!! rust "Lint que ajuda"
    `unsafe_op_in_unsafe_fn = "deny"` (ver o [`Cargo.toml` do workspace](projecto-completo.html#lints-num-so-sitio)) obriga cada operação insegura, mesmo dentro de uma `unsafe fn`, a ter o seu próprio bloco `unsafe` — e portanto o seu próprio `SAFETY`.

## Iteradores em vez de laços com estado

```rust
/// Soma a memória pedida dos containers que vão correr, ignorando os sem limite.
pub fn total_memory(specs: &[RunSpec]) -> u64 {
    specs.iter().filter_map(|s| s.memory).sum()
}
```

Sem índices, sem acumulador mutável, sem *off-by-one*. `filter_map` combina «filtra os `None`» e «desembrulha os `Some`». Os iteradores são abstracções de **custo zero**: o compilador gera o mesmo código que o laço à mão.

## Ferramentas que a comunidade espera de ti

```bash
cargo fmt --check                                   # formatação canónica: zero discussões de estilo
cargo clippy --all-targets -- -D warnings           # centenas de lints: apanha bugs, não só estilo
cargo test --doc                                    # os exemplos da documentação são testes
cargo deny check                                    # licenças e advisories das dependências
```

Neste tutorial, tudo isto corre em CI ([`.github/workflows/ci.yml`](projecto-completo.html#ci)) — e é o mesmo conjunto que o delonix impõe antes de aceitar um PR.

## Verifica o que aprendeste

```bash
cargo test -p examples ch02        # 3 testes + 1 doctest + 2 compile_fail
```

Exercício: acrescenta o estado `Paused` ao typestate (`Running → Paused → Running`). Que métodos precisas de escrever? Que *não* deves escrever (por exemplo, `stop` em `Container<Paused>`)?
