---
title: Contribuir para o delonix-runtime
slug: contribuir
summary: Do issue ao PR fundido — worktree, os gates de CI, ADRs, alinhamento de versão e o que os revisores procuram.
part: 5
order: 17
time: 20 min de leitura
---

# Contribuir para o delonix-runtime

Já sabes ler o motor (capítulos 1–8) e construíste o teu núcleo (9–16). Este capítulo é o **método** para enviar a primeira mudança sem partir um *gate* nem o trabalho de outra pessoa. É um resumo orientado; a fonte autoritativa é o **manual do contribuidor** do próprio repositório, em [`docs/dev/`](https://github.com/angolardevops/delonix-runtime/tree/main/docs/dev) — começa por [`start-here.md`](https://github.com/angolardevops/delonix-runtime/blob/main/docs/dev/start-here.md) e [`contributing-workflow.md`](https://github.com/angolardevops/delonix-runtime/blob/main/docs/dev/contributing-workflow.md).

!!! tip "Antes de qualquer coisa não trivial"
    Um comando novo, um *Kind* novo, uma mudança na criação de namespaces ou cgroups, um backend novo: **abre um issue primeiro** e combina a abordagem. Poupa uma reescrita. E se a mudança altera uma **fronteira estrutural**, precisa de um ADR escrito antes do código.

## Por onde começar (bons primeiros contributos)

Do menor risco para o maior:

1. **Documentação e traduções** — `docs/dev/` tem versões `pt-AO` e `fr-FR`; uma correcção ou uma página traduzida é um PR real e seguro.
2. **Testes que faltam** — procura um caminho sem cobertura na CLI (a bateria `scripts/e2e.sh` mede a fracção de folhas executadas *vs* só verificadas com `--help`).
3. **Mensagens de erro que não dizem o remédio** — uma mensagem que diz *o que* falhou sem dizer *o que fazer* é um bug de UX.
4. **Um exercício deste tutorial** — [`seccomp`](mc-proximos-passos.html#exercicio-3-seccomp-medio-dificil) ou `exec` no `minicontainer` ensinam exactamente o código que vais ler.
5. **Um *Kind* novo** — [`docs/dev/adding-a-kind.md`](https://github.com/angolardevops/delonix-runtime/blob/main/docs/dev/adding-a-kind.md) é um passo-a-passo (mas veja-se que um Kind toca em **várias tabelas**, cada uma com o seu gate).

## O fluxo

### 1. Parte do último estado — e do histórico da zona

```bash
git clone https://github.com/angolardevops/delonix-runtime.git && cd delonix-runtime
git fetch --tags origin
git describe --tags --abbrev=0 origin/main                     # a release mais recente
git log --oneline -- <o caminho que vais tocar>                # o que já foi decidido, corrigido ou removido
```

Ler o histórico da zona **não é cerimónia**: uma parte grande deste código é o registo de coisas tentadas, medidas e mudadas. O que já foi decidido ou removido não se refaz por desconhecimento.

### 2. Um worktree por tarefa

Várias pessoas e ferramentas trabalham no mesmo clone ao mesmo tempo, e editar numa árvore partilhada já custou trabalho a sério (edições absorvidas no commit de outra pessoa; o `HEAD` a mudar de ramo a meio de uma tarefa; um `cargo test` verde numa árvore suja que não provava que o `HEAD` compilava). Por isso:

```bash
git fetch origin
git worktree add -b <tema>/<tarefa> ../.worktrees/delonix-runtime/<tarefa> origin/main
cd ../.worktrees/delonix-runtime/<tarefa>
```

- **Nunca** ponhas o worktree em `/tmp` — muitos sistemas esvaziam-no no arranque, e um reinício a meio leva trabalho por commitar.
- **Commit e push cedo**, a cada passo que passa as verificações.
- **`git add <ficheiro> <ficheiro>`**, nunca `-A`, `-u` ou `.`: absorvem trabalho alheio.
- **Nunca `git checkout -- <caminho>`** numa árvore que outra pessoa possa estar a usar: reverte sem *stash*.
- No fim, **remove o worktree e o ramo** (`git worktree remove <path>` e `git branch -D <ramo>`) — o ramo sobrevive ao worktree, e é assim que o lixo se acumula.

### 3. Compila e testa **o teu** binário

```bash
cargo build --workspace
cargo test  --workspace
./target/debug/delonix --version      # NUNCA o `delonix` do PATH: é uma release instalada, muitas vezes atrasada
```

### 4. Os gates que a CI corre

| Gate | O que recusa |
|---|---|
| `cargo fmt --check` · `clippy -D warnings` | estilo e bugs comuns |
| `scripts/arch_fitness.py` | uma dependência contra a direcção das camadas; um crate fora do directório da sua camada; o **nome de um consumidor** no código |
| `scripts/lang_ratchet.py` | português novo no código (identificadores, comentários, mensagens) — é um *ratchet* |
| `scripts/contract_gate.py` | o contrato de nó (`proto/`) fora de `buf format`/`lint`, quebra de compatibilidade, ou o OpenAPI diferente do gerado |
| `scripts/version_gate.py` | a versão do `Cargo.toml` desalinhada da última tag |
| testes de *help* e i18n | um comando ou flag sem entrada `pt.po` |

### 5. Regras que os revisores aplicam

- **Código, comentários e mensagens em inglês.** O português chega ao operador por `po::t(...)` e o catálogo `pt.po`. Quando traduzires algo, `python3 scripts/lang_ratchet.py --update` e a nova linha de base vão **no mesmo commit**.
- **O motor não conhece consumidores.** Nada de nomes de plataformas, *tenants*, quotas, facturação — nem em comentários. Um requisito de um consumidor escreve-se como a capacidade que é.
- **Nada aceite e ignorado.** Uma opção que o utilizador escreve e o motor deixa cair em silêncio é pior que uma que não existe.
- **Prova medida, não afirmada.** Uma correcção vem com o **teste que falha sem ela** e, quando toca no kernel, com a validação ao vivo.
- **Sem dependências novas sem razão.** A árvore é pequena de propósito.
- **Não mexas na versão** num PR normal: fica igual à última tag; só o *commit* de release a sobe (e com `docs/releases/v<versão>.md`).

### 6. O PR

Empurra a tua branch e abre o PR; **não** se empurra directamente para a `main`. Descreve **o quê**, **porquê**, **como verificaste** e — a parte que faz a diferença — **o que não validaste**:

```markdown
## O que muda
## Porquê (e o que estava errado antes — com a medição)
## Como verifiquei
- cargo test --workspace ✔
- validado ao vivo: <o comando e o que devolveu>
## O que NÃO foi validado
- <ex.: só testado em kernel 7.0; sem GPU real>
```

Este último bloco é a regra da casa: **reporta as duas metades**. Um relatório só com a parte boa é um relato desonesto.

## ADRs: quando escrever um

Escreve um **ADR** (*Architecture Decision Record*) quando a mudança:

- altera uma **fronteira estrutural** (camadas, o que depende do quê);
- introduz um **backend**, uma **dependência pesada** ou — sobretudo — um **daemon**;
- vai contra uma regra escrita (daemonless, rootless-first, o motor não conhece consumidores).

Um ADR aceite ganha a qualquer opinião posterior; substitui-se **com outro ADR**, nunca com uma frase noutro sítio. Fronteiras novas (um backend, um provider) passam por um *spike* com **GO/NO-GO** antes do código. Os existentes estão em [`docs/adr/`](https://github.com/angolardevops/delonix-runtime/tree/main/docs/adr).

## Segurança

O motor corre como o teu utilizador sobre input hostil (imagens, manifestos, nomes). Antes de tocares numa **fronteira de privilégio** (execução remota, SSH, *build* de imagens, montagens), relê os capítulos [12](mc-namespaces-rootfs.html) e [13](mc-cgroups-caps.html) e a checklist de [Projecto completo](projecto-completo.html#seguranca-habitos-nao-auditorias). Uma vulnerabilidade deve ser reportada de forma privada (`SECURITY.md` do repositório), não como issue público.

## Onde pedir ajuda

- **Issues** no repositório do delonix — para bugs e propostas.
- **O manual do contribuidor** (`docs/dev/`) — ambiente, *build*, cada crate, o *system design* do motor, resolução de problemas (`troubleshooting.md`).
- **Este tutorial**: encontraste um erro? Cada página tem «Sugerir uma correcção» no fim.

!!! delonix "A ideia por trás de tudo isto"
    As regras parecem muitas, mas têm todas a mesma origem: cada uma é a cicatriz de algo que **já custou caro** — trabalho perdido numa árvore partilhada, um digest decorativo, um `flock` esquecido, um teste que passava com o código apagado. Segue-as por essa razão, não por burocracia; e quando descobrires uma nova, escreve-a — é assim que a lista cresce.
