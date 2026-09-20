---
title: Rust essencial
slug: rust-essencial
summary: Ownership, enums, Result e traits com exemplos de um motor de containers — todos compilados e testados.
part: 1
order: 1
time: 25 min de leitura
---

# Rust essencial

Não vais aprender Rust inteiro aqui — vais aprender **o suficiente para ler o motor** e escrever a tua primeira contribuição. Cada exemplo abaixo vive em [`examples/src/ch01_essencial.rs`](https://github.com/angolardevops/delonix-rust-tutorial/blob/main/examples/src/ch01_essencial.rs) e corre em CI (`cargo test -p examples`).

!!! rust "Porque Rust para um runtime de containers?"
    Um runtime corre como utilizador privilegiado sobre input hostil (imagens, manifestos, nomes) e fala com o kernel por chamadas de sistema. Rust dá-te **segurança de memória sem *garbage collector*** (sem pausas, sem runtime a carregar), erros que **têm** de ser tratados, e um sistema de tipos capaz de tornar estados inválidos *impossíveis de escrever*. O delonix escolheu-o por isso — e por ser um único binário estático, sem daemon.

## Ownership e empréstimos

Cada valor tem **um dono**. Passar por valor **move**; passar por referência (`&`) **empresta**. O compilador prova que nunca há dois donos, nem uma referência a algo já libertado — a classe de bugs (*use-after-free*, *double free*) que domina as CVEs de runtimes em C.

{{file:examples/src/ch01_essencial.rs#ownership}}

!!! tip "Boa prática"
    Recebe `&str` e `&[T]`, não `&String` nem `&Vec<T>`: aceitam mais tipos e não custam nada. Só recebe `String` por valor quando a função **precisa de ficar dona** (guardar num struct, mover para uma thread).

Para mutar, `&mut T` é **exclusivo**: enquanto existe, mais ninguém pode ler nem escrever. É por isto que, em Rust, «corridas de dados» são erro de compilação.

## Enums e `match` exaustivo

Um `enum` de Rust é uma *soma* de variantes que **carregam dados**. Modela um estado como este e o compilador obriga-te a tratar todos os casos:

{{file:examples/src/ch01_essencial.rs#enums}}

Se acrescentares `Paused` a `Status`, `describe` **deixa de compilar** até decidires o que fazer. Compara com um `switch` com `default:` que engole o caso novo em silêncio.

!!! delonix "No delonix"
    O `Status` do container é um enum, e o motor vai mais longe: o [typestate](rust-idiomatico.html#typestate-transicoes-ilegais-que-nao-compilam) faz das transições ilegais *erros de compilação*.

## `Result`, `Option` e o operador `?`

Rust não tem excepções. Uma função que pode falhar devolve `Result<T, E>`; uma que pode não ter valor devolve `Option<T>`. O `?` propaga o erro ao chamador:

{{file:examples/src/ch01_essencial.rs#result}}

Repara em três decisões que vais ver por todo o código do motor:

1. **Erros como valores** (`ParseError`), não *strings* — o chamador pode fazer `match` e decidir.
2. **`saturating_mul`** em vez de `*`: input hostil (`9999999999999G`) não dá volta ao inteiro em silêncio. O delonix apanhou um bug real desta família — `as u64` sobre um `f64` **satura** em Rust, e uma quota de «99999999999t» tornou-se `u64::MAX`, ou seja, *quota nenhuma*, com o `inspect` a mostrá-la como definida.
3. **Nada de `unwrap()` em código de produção** — um pânico num runtime derruba a máquina de um cliente. (Nos testes, `unwrap` é aceitável: o pânico *é* a falha.)

## Traits: o padrão «backend»

Um `trait` é uma interface. O motor usa-o para falar com hypervisors sem `if provider == "libvirt"` espalhado pelo código:

{{file:examples/src/ch01_essencial.rs#traits}}

Dois modos de usar, com trade-offs diferentes:

| | Genérico `fn f<B: Backend>` | Objecto `Box<dyn Backend>` |
|---|---|---|
| Despacho | estático (inlined, custo zero) | dinâmico (*vtable*) |
| Quando | o tipo conhece-se em compilação | escolhido em *runtime* (um **registo** de backends) |
| Custo | um binário maior (monomorfização) | uma indirecção por chamada |

E o código real, no motor — o trait `VmBackend`, que faz o mesmo com Cloud Hypervisor, libvirt e Proxmox:

{{snippet:vm-backend}}

Repara no `ip_is_predicted`: um **método com implementação por omissão**, sobrescrito só pelo backend cujo IP é *calculado* e não *observado*. O conhecimento fica onde pertence — no backend — em vez de um `if` no sítio da chamada.

## Módulos, crates e workspaces

- **Módulo** (`mod`) — organização dentro de um crate; a visibilidade por omissão é **privada**.
- **Crate** — a unidade de compilação (biblioteca ou binário).
- **Workspace** — vários crates com um só `Cargo.lock` e um só `target/`. O delonix tem **22**, e uma tabela imposta por CI diz quem pode depender de quem — ver [Anatomia do delonix](anatomia-delonix.html).

```toml
# Cargo.toml (raiz) — como este tutorial organiza os seus dois crates
[workspace]
resolver = "3"
members = ["minicontainer", "examples"]
```

## Verifica o que aprendeste

```bash
cargo test -p examples ch01      # 4 testes — os que acabaste de ler
```

Antes de continuares, tenta (sem espreitar) prever o que faz `parse_size("-1")` e porquê `parse_size("9999999999999G")` **não** dá pânico. As respostas estão nos testes.

!!! tip "Para ir mais longe"
    O manual do contribuidor do próprio repositório tem uma cartilha de Rust dirigida ao motor: [`docs/dev/rust-primer.md`](https://github.com/angolardevops/delonix-runtime/blob/main/docs/dev/rust-primer.md). E o livro oficial — *The Rust Programming Language* — é a referência canónica para tudo o que aqui só se tocou.
