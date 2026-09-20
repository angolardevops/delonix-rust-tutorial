---
title: Um projecto Rust completo
slug: projecto-completo
summary: Workspace, lints num só sítio, testes em pirâmide, CI, cargo-deny, semver e ADRs — as recomendações da comunidade aplicadas a um projecto real.
part: 3
order: 8
time: 30 min de leitura
---

# Um projecto Rust completo

Escrever código que funciona é o primeiro terço. Os outros dois são **mantê-lo** e **deixar outros mexerem-lhe sem medo**. Este capítulo é a lista de decisões de um projecto de sistemas em Rust que a comunidade reconhece como saudável — e cada uma está aplicada **neste repositório**, que podes clonar e usar como *template*.

## Estrutura

```text
delonix-rust-tutorial/
├── Cargo.toml              # workspace + lints partilhados + perfil de release
├── rustfmt.toml  clippy.toml  deny.toml
├── minicontainer/          # o projecto (biblioteca + binário `mc`)
│   ├── src/  lib.rs  main.rs  error.rs  spec.rs  image.rs  container.rs  cgroup.rs  state.rs  fsutil.rs
│   └── tests/e2e.rs        # testes de integração: correm o binário a sério
├── examples/               # cada exemplo dos capítulos, compilado e testado
├── scripts/                # demo.sh, make-rootfs.sh, extract-snippets.py …
├── content/  site-src/  build.py     # este site
└── .github/workflows/      # ci.yml (qualidade) e pages.yml (publicação)
```

Três decisões a copiar:

1. **Biblioteca + binário fino.** `lib.rs` tem a lógica (testável, reutilizável); `main.rs` só faz *parse* de argumentos e traduz erros em códigos de saída. Se a lógica vivesse no `main`, só se testaria por subprocesso.
2. **`tests/` para o que é do binário**, `#[cfg(test)]` para o que é da função. Os testes de integração usam `env!("CARGO_BIN_EXE_mc")` — o binário **real**.
3. **Um crate `examples`** para o código dos capítulos: se um exemplo do tutorial estiver errado, o CI fica vermelho. Documentação que não compila mente.

## Lints num só sítio

{{file:Cargo.toml}}

- `[workspace.lints]` — definidos **uma vez**, herdados com `[lints] workspace = true` em cada crate. Sem isto, cada crate diverge.
- `unwrap_used = "warn"` — combinado com `clippy.toml` (`allow-unwrap-in-tests = true`): **proibido em produção, livre nos testes**.
- `panic = "abort"` no *release* — um runtime que faz `fork`/`exec` não faz *unwinding* através deles.
- `unsafe_op_in_unsafe_fn = "deny"` — cada operação insegura tem o seu bloco e o seu `// SAFETY:`.

!!! rust "Edição, MSRV e `resolver`"
    `edition = "2024"` e `resolver = "3"` são o presente; `rust-version` (o **MSRV**, versão mínima suportada) declara-se para o Cargo recusar compilar com um *toolchain* antigo com uma mensagem clara, em vez de um erro críptico a meio.

## A pirâmide de testes

| Nível | Onde | O que prova | Neste repositório |
|---|---|---|---|
| **Unidade** | `#[cfg(test)]` | uma função pura, em microssegundos | `spec`, `cgroup::limit_files`, `reconcile` |
| **Documentação** | `///` com ```` ``` ```` | que o exemplo compila **e** o contrato | typestate com `compile_fail` |
| **Propriedade/concorrência** | `#[cfg(test)]` | invariantes sob entradas/corridas | `state`: 16 *threads* a actualizar |
| **Integração** | `tests/` | o **binário**, com o kernel a sério | `e2e.rs`: PID 1, exit codes, capabilities |
| **Conformidade** | fora do repo | outro implementador cumpre a mesma norma | o mesmo bundle no `runc` |
| **Caos** | script | sobrevive a falhas injectadas | (no delonix: `scripts/chaos.sh`) |

Regras que o delonix aprendeu a pagar, e que os testes deste tutorial seguem:

!!! warning "Um teste que passa com o código apagado não prova nada"
    Antes de confiar num teste, **reverte a correcção e vê-o falhar**. O delonix chama a isto «verificado pela regra do repo»: um cenário de caos sobre convergência tinha a asserção «o PID não mudou», que um `apply` que não faz nada também satisfaz. A asserção certa observa o **efeito** (o registo mudou *e* o plano seguinte não tem nada a propor).

!!! warning "«Passou» não é «correu»"
    Os testes E2E deste repositório **saltam com aviso** quando o host não permite user namespaces — mas um salto silencioso lê-se como verde. Por isso o salto é dito (`SKIP: user namespaces indisponíveis`), e no delonix os *runners* alojados do GitHub bloqueiam userns e o job de caos fica «verde a saltar tudo»: um verde por ausência de execução é indistinguível de um verde por sucesso se só se olhar para o topo.

Um teste de **concorrência** a sério não pode ser óbvio. Este falha sem o `flock`:

{{file:minicontainer/src/state.rs#concurrent-test}}

## CI: o que corre em cada PR {#ci}

{{file:.github/workflows/ci.yml}}

Quatro *gates* independentes: **formatação** (zero discussões de estilo), **clippy com `-D warnings`** (avisos são erros), **testes** (incluindo *doc tests*), e **`cargo deny`** (licenças, *advisories* e fontes das dependências). O job `site` garante que **nenhum link interno do tutorial está partido**.

{{file:deny.toml}}

!!! tip "Fixa o que corre, não só o que compila"
    Usa `cargo deny` para **recusar** dependências com licença inesperada ou vulnerabilidade conhecida, e mantém a árvore pequena: cada dependência é superfície de ataque. O delonix confina dependências pesadas (`ratatui`, `schemars`, `hyper`) ao crate que precisa e **verifica** com `cargo tree -e normal -p <crate>` que os crates de mecanismo continuam limpos.

## Documentação que se mantém

- **`///` com exemplos** — testados por `cargo test --doc`.
- **`//!` no topo de cada módulo** — *porquê* o módulo existe, não *o que* faz (isso lê-se no código). Vê os do `minicontainer`: quase todos explicam uma decisão ou uma armadilha.
- **ADRs** (*Architecture Decision Records*) — um ficheiro por decisão de fronteira, com contexto, decisão e consequências. Uma decisão sem ADR é uma opinião. O delonix tem dezenas em `docs/adr/`.
- **Comentários que dizem o que foi medido**: «*MEASURED 2026-09-15, k8s 1.36.4, …*». Um comentário sem evidência envelhece; um com a medição pode ser contestado.

## Versões e releases

- **SemVer**: `MAJOR.MINOR.PATCH`, e no Rust a API pública é o contrato — `cargo semver-checks` apanha quebras acidentais.
- **A versão no `Cargo.toml` está sempre alinhada com a última tag publicada** (o delonix impõe-o com `version_gate.py`): «duas builds com a mesma versão não são a mesma build», e uma versão «de trabalho» que já não corresponde a nenhuma tag confunde quem reporta bugs.
- **Perfil de release** com `lto = "thin"`, `codegen-units = 1` para um binário menor e mais rápido.
- **Reprodutibilidade**: `Cargo.lock` **commitado** para binários e aplicações.

## Segurança: hábitos, não auditorias

1. **Valida na fronteira** com tipos ([newtype](rust-idiomatico.html#newtype-validar-uma-vez-a-entrada)), não com `if` espalhados.
2. **Recusa, não ignores**: um campo aceite e ignorado é pior que um campo que não existe (é a regra que o `Spec::validate` do `minicontainer` cumpre para o `seccomp`).
3. **`unsafe` mínimo e comentado.**
4. **Verifica o que descarregas** (digest, checksum) antes de o usares, e falha **fechado**.
5. **Sem segredos em argumentos** (visíveis no `ps`): ficheiro com `0600` ou `stdin`.
6. **Ficheiros temporários** com `O_EXCL` e nome único, nunca um caminho fixo em `/tmp` (o delonix teve uma escalada de privilégio local exactamente por escrever `/tmp/x.o` para o `bpftool` root carregar).

## A checklist

- [ ] Workspace com lints partilhados e `rustfmt.toml`/`clippy.toml`
- [ ] Biblioteca + binário fino; erros tipados na biblioteca
- [ ] Newtypes para tudo o que entra em caminhos, comandos ou nomes
- [ ] `unsafe` só onde inevitável, sempre com `// SAFETY:`
- [ ] Testes: unidade, doc, integração — e pelo menos **um** que prove o efeito, não o retorno
- [ ] CI: `fmt`, `clippy -D warnings`, `test`, `deny`
- [ ] `--help` e mensagens de erro que dizem **o remédio**
- [ ] ADR para cada decisão de fronteira
- [ ] Nada aceite e ignorado em silêncio
