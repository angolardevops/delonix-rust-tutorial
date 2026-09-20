---
title: Anatomia do delonix-runtime
slug: anatomia-delonix
summary: Os 22 crates em 5 camadas, a regra de dependências imposta por CI, e os princípios daemonless e rootless-first.
part: 3
order: 7
time: 30 min de leitura
---

# Anatomia do delonix-runtime

O **delonix-runtime** é um motor de **containers e microVMs** para um nó: daemonless, rootless-first, escrito em Rust, sob Apache-2.0 ([repositório](https://github.com/angolardevops/delonix-runtime)). É o projecto para o qual este tutorial te prepara a contribuir. Estas são as medidas do commit que os excertos deste site citam (`a7b017c7`, depois da v4.1.0):

| | |
|---|---|
| Crates | **22** em 5 camadas + 3 binários de aplicação |
| Ficheiros Rust | 209 (`crates/` e `bins/`) |
| Maiores crates | `delonix-sdn` ~18 k linhas · `delonix-linux` ~15,5 k · `delonix-vm` ~9,6 k · `delonix-cri` ~7,4 k · `delonix-oci` ~7,3 k |
| Binários | `delonix`, `delonix-cri`, `delonix-mcp`, `delonix-mgmt` |

## Os três princípios (e o que cada um proíbe)

1. **Cloud native** — declarativo e convergente (plano, *apply*, deriva), API-first (a CLI, a API de nó, o CRI e o MCP expõem as mesmas operações), observável por padrões abertos. *Proíbe:* estado escondido num processo.
2. **Daemonless** — **nenhum processo residente por omissão.** O que precisa de persistir é do systemd (unit, *timer*, *socket activation*) ou de um processo por *workload* com dono claro (o supervisor de um container, o *holder* de rede). *Proíbe:* um daemon central; exige um ADR com a evidência do que a alternativa não resolveu.
3. **Rootless-first** — o caminho normal corre sem root; privilégio é *opt-in* explícito e dito ao operador, nunca um *default* silencioso.

É exactamente o que o `minicontainer` faz em ponto pequeno: sem daemon (um supervisor por container, estado em disco), sem root (user namespace), e recusa o que não sabe cumprir.

!!! delonix "O motor não conhece nenhum consumidor"
    Uma regra que surpreende: o motor **não sabe quem o usa**. Não conhece plataformas, control planes, consolas nem *tenants*, quotas ou facturação — nem nos comentários. Quem consome adapta-se aos contratos do motor, não o contrário; um pedido de um consumidor escreve-se como **a capacidade que é**, no vocabulário do motor. Um script (`arch_fitness.py`) **falha** se o nome de um consumidor aparecer no código.

## As 5 camadas

{{svg:delonix-camadas}}

| Camada | Crates | Papel |
|---|---|---|
| **foundation** | `delonix-model`, `delonix-net-rules` | Tipos puros que qualquer camada nomeia: erros e códigos `DX-CDNN`, registos, o typestate, regras de rede puras (zero dependências) |
| **contexts** | `delonix-stack`, `delonix-compute`, `delonix-node`, `delonix-security-runtime` | O domínio: o reconciliador, a especificação de execução, o contexto do nó, as decisões de segurança |
| **adapters** | `delonix-linux`, `-sdn`, `-oci`, `-vm`, `-volume`, `-state`, `-scanner`, `-telemetry` | O mundo real: kernel, rede, imagens, VMs, disco |
| **providers** | `delonix-proxmox`, `delonix-truenas` | Sistemas remotos, sempre atrás de uma **porta** |
| **interfaces** | `delonix-cri`, `delonix-mgmt`, `delonix-mcp` | Formas de falar com o motor: CRI, API local, servidor MCP |

Os binários (`bins/`) **compõem uma só interface** cada — é essa regra que impede o `delonix` de voltar a carregar quatro servidores enquanto os servidores lhe voltam a chamar por subprocesso.

## A arquitectura é um teste

Uma regra de arquitectura que só existe num documento apodrece. No delonix é **código que corre em CI**: `scripts/arch_fitness.py` tem uma tabela de camadas e uma tabela de direcções permitidas, e chumba o build se um crate depender contra a corrente.

{{snippet:arch-layers}}

Lê o `ALLOWED`: a fundação só depende da fundação; um contexto **não** depende de adaptadores; um adaptador **não** depende de interfaces. Cada excepção tem de **nomear a fase que a remove** — uma excepção sem fase falha o portão, e uma que já não se aplica também. É assim que uma tolerância temporária deixa de ser permanente.

Dois números são **ratchets** (falham se *sobem* **e** se *descem* sem a linha de base baixar no mesmo commit):

- `self_exec_sites` — quantas vezes uma biblioteca volta a correr o binário do próprio motor em vez de chamar uma função (o ciclo escondido atrás de um processo);
- `library_prints` — `println!` em crates de biblioteca (uma biblioteca emite `tracing`; quem imprime é a interface).

!!! tip "Um ratchet, não um tecto"
    Um `<=` deixaria a dívida a ler-se como «verde» para sempre. Um ratchet força a **descer** a linha de base no mesmo commit em que se paga a dívida — a melhoria fica registada e não regride. Aplica-se o mesmo à língua do código: `lang_ratchet.py` conta identificadores, comentários e mensagens ainda em português. **Todo o código do motor é em inglês**; o português chega ao operador por um catálogo de tradução (`pt.po`).

## Um pedido, ponta a ponta: `delonix container run`

O que acontece num `delonix container run -d alpine sleep 100`, e onde cada peça vive:

1. **`delonix-runtime-bin`** — a CLI faz *parse* (clap), valida e produz uma `RunOpts` (`delonix-compute`), a **especificação de execução única** que a CLI, o compose, a API Docker e o CRI produzem antes de um só caminho a executar. A validação pura (`preflight::check_run_opts`) corre antes de qualquer efeito.
2. **`delonix-oci`** — resolve a imagem no armazém por conteúdo; se faltar, *pull* verificado por digest.
3. **`delonix-state`** — o registo do container, com `flock` e escrita atómica ([capítulo 6](sistemas-distribuidos.html)).
4. **`delonix-linux`** — `spawn`: `clone` com os namespaces, *handshake* de user namespace, `pivot_root`, cgroup, capabilities, seccomp; e o **supervisor** que fica a ver o container morrer.
5. **`delonix-sdn`** — o *holder* de rede (uma netns que sobrevive ao plano de controlo), bridge, `nftables`, DNS.

Precisa do `minicontainer` para ver o passo 4 a sério — é o que fazes a partir do [capítulo 9](mc-visao.html).

## Como o estado é organizado

Sem daemon, **o estado vive em disco** (a raiz de estado, configurável por `DELONIX_ROOT`): um JSON por container, por volume, por rede — lidos e escritos com `flock` e escrita atómica. Isto tem consequências que o motor documenta e o `minicontainer` reproduz em pequeno:

- Um processo pode morrer a meio: **o estado em disco pode mentir** (diz `Running` para um PID morto). Por isso existe `reconcile_status`, que confronta o ficheiro com a realidade ([`reconcile`](mc-ciclo-de-vida.html)).
- Estado necessário para **reconstruir** o recurso tem de ser persistido, não só usado na criação. Já morderam três vezes: `-v` não persistido (um `start` voltava a correr **sem** os volumes, escrevendo no rootfs), `-p` em rede custom, e as redes adicionais.

## Erros com código

Cada erro traz um código do dicionário `DX-CDNN` (`delonix-model`) e um **código de saída com classe** (4 = não existe, 5 = conflito, 69 = capacidade em falta do host, 77 = permissão…), decidido **num só sítio** (`for_error`, o excerto do [capítulo 2](rust-idiomatico.html)). É a mesma ideia do `Error::exit_code` do `minicontainer`.

## Onde procurar cada coisa

```text
delonix-runtime/
├── crates/
│   ├── foundation/   delonix-model, delonix-net-rules
│   ├── contexts/     delonix-stack, -compute, -node, -security-runtime
│   ├── adapters/     delonix-linux, -sdn, -oci, -vm, -volume, -state, -scanner, -telemetry
│   ├── providers/    delonix-proxmox, delonix-truenas
│   └── interfaces/   delonix-cri, -mgmt, -mcp
├── bins/             delonix-runtime-bin, delonix-mcp-bin, delonix-mgmt-bin
├── docs/             adr/ (decisões), dev/ (manual do contribuidor), api/ (OpenAPI gerado)
├── proto/            o contrato de nó (gRPC + HTTP/JSON), fonte de verdade
└── scripts/          arch_fitness.py, lang_ratchet.py, contract_gate.py, version_gate.py, e2e.sh, chaos.sh
```

!!! tip "Documentação para quem contribui"
    O repositório tem um **manual do contribuidor** em `docs/dev/` (ambiente, *build* e *gates*, um capítulo por crate, o *system design* do motor) e **ADRs** em `docs/adr/` que registam cada decisão de fronteira. Lê o ADR antes de propor mudar uma fronteira — o que já foi decidido, corrigido ou removido não se refaz por desconhecimento.
