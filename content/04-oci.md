---
title: OCI — imagem e runtime
slug: oci
summary: As duas normas que tornam os containers portáteis — image-spec e runtime-spec — com um bundle real corrido no minicontainer e no runc.
part: 2
order: 4
time: 30 min de leitura
---

# OCI — imagem e runtime

A **Open Container Initiative** define duas normas independentes. Perceber que são **duas** é metade do trabalho:

| Norma | Responde a | Artefacto |
|---|---|---|
| **image-spec** | *Como se empacota e distribui uma imagem?* | *image layout*: `index.json`, manifest, config, layers |
| **runtime-spec** | *Como se corre um container a partir de uma pasta?* | *bundle*: `config.json` + `rootfs/`, e um ciclo de vida |

Entre as duas há uma ponte, e é o trabalho de qualquer ferramenta tipo `umoci`/`skopeo`/`docker`: **desempacotar** a imagem num *bundle*. Um runtime de baixo nível (`runc`, `crun`, o `minicontainer`) só conhece o bundle.

{{svg:oci-fluxo}}

## image-spec: o que é uma imagem

Uma imagem é um **grafo de blobs endereçados pelo seu hash**. No disco (*image layout*):

{{out:11-imagem-oci}}

Do topo para baixo: `index.json` aponta para um **manifest**; o manifest aponta para uma **config** e para uma lista ordenada de **layers**. Cada ponteiro é um *descritor* `{mediaType, digest, size}`:

{{out:14-oci-json}}

Três ideias a reter:

1. **Endereçamento por conteúdo.** O nome do ficheiro em `blobs/sha256/` *é* o SHA-256 do seu conteúdo. Dois ficheiros iguais são um só; um ficheiro alterado tem outro nome. É por isto que uma imagem se pode partilhar entre imagens e verificar.
2. **Layers são *diffs* de sistema de ficheiros**, empilhados por ordem. A layer 2 pode acrescentar ficheiros, sobrescrever, e **apagar** — com um *whiteout*.
3. **A config não corre nada.** `Entrypoint`, `Cmd`, `Env`, `WorkingDir` são só o que a imagem *sugere*; quem decide é o bundle.

### Whiteouts: como se apaga numa camada só de adições

Um `tar` só sabe acrescentar. Para a layer 2 «apagar» `/etc/removido`, contém um ficheiro vazio chamado `.wh.removido` no mesmo directório (e `.wh..wh..opq` para *esvaziar* um directório inteiro). No output acima: `ls bundle2/rootfs/etc` mostra só `motd` e `passwd` — o ficheiro da layer 1 foi apagado pelo whiteout da layer 2.

{{file:minicontainer/src/image.rs#layer}}

### Verificar tudo, antes de escrever

O `unpack` do `minicontainer` verifica **cada blob contra o digest que o referencia** antes de o usar — e verifica **todas** as layers antes de escrever um único byte no bundle:

{{file:minicontainer/src/image.rs#verified}}

Adultera um byte e a extracção **recusa**, sem deixar lixo:

{{out:12-digest-adulterado}}

E o formato do digest é validado *antes* de tocar no disco — um `digest` como `sha256:../../etc/passwd` sairia do layout:

{{file:minicontainer/src/image.rs#blob-path}}

!!! delonix "O digest decorativo — uma auditoria real"
    O delonix já teve um **ALTO** exactamente aqui, um nível acima: `pull repo@sha256:X` verificava cada **blob** contra o que o *manifest* declarava, mas nunca o **manifest** contra o digest pedido. Um registo comprometido devolvia um manifest totalmente diferente, internamente consistente, e o motor instalava o conteúdo do atacante sem um erro. O *pin* era decorativo. A correcção:

{{snippet:verify-digest}}

## runtime-spec: como se corre uma pasta

O **bundle** é uma pasta com `config.json` e o rootfs. O `config.json` diz *tudo* o que o runtime tem de aplicar:

{{out:15-config-json}}

Cada campo corresponde a um mecanismo do [capítulo anterior](linux-containers.html):

| Campo | Mecanismo |
|---|---|
| `root.path`, `root.readonly` | `pivot_root` (+ remount só de leitura) |
| `mounts[]` | `mount(2)` — `proc`, `tmpfs`, *bind* |
| `hostname` | `sethostname` no `uts` namespace |
| `linux.namespaces[]` | quais `unshare` |
| `linux.resources` | ficheiros do cgroup v2 |
| `process.args/env/cwd` | o `exec` final |
| `linux.seccomp`, `process.capabilities` | filtro e conjunto limitador |

### O ciclo de vida

A runtime-spec fixa **cinco operações** e **quatro estados**. É isto que o `runc`, o `crun` e o `minicontainer` partilham — e é o que uma camada acima (o CRI) invoca:

```text
                create               start                 (o processo sai / kill)
   (nada) ──────────────▶ created ──────────────▶ running ────────────────────▶ stopped ── delete ──▶ (nada)
                    │                                                              ▲
                    └── creating (transitório: o runtime está a preparar)          │
   state <id>  → JSON {ociVersion, id, status, pid, bundle}      kill <id> <sinal> ─┘
```

A separação `create` / `start` não é decorativa: entre as duas o container **existe** (namespaces, mounts, cgroup prontos) mas o processo do utilizador **ainda não correu**. É a janela em que o orquestrador liga a rede, corre *hooks* e só então dá o tiro de partida.

## Contra-prova: o mesmo bundle corre no `runc`

Um runtime que só corre os *seus* bundles não cumpre a norma. O bundle que o `minicontainer` acabou de correr, entregue ao `runc` 1.5.1 (só com os mapeamentos de user namespace que o `runc` exige explícitos):

{{out:13-runc-mesmo-bundle}}

Mesma saída. **Isto é a prova de conformidade que interessa**: dois runtimes independentes, uma norma.

!!! warning "O que esta prova NÃO cobre"
    Corre-se **um** bundle. A suite oficial (`opencontainers/runtime-tools`, `validation`) testa centenas de casos e o `minicontainer` **não** a passa toda — recusa por desenho o que não implementa (seccomp, terminal, juntar-se a namespaces existentes). Está dito no código e testado: [um bundle que peça seccomp é recusado, não ignorado](mc-testes.html).

## Do que o delonix é feito

O delonix não delega o «desempacotar» a outra ferramenta: o crate `delonix-oci` faz *pull* de registos, gere o armazenamento por conteúdo (CAS) e as layers, e os containers rootless **partilham as layers** por `overlayfs` montado dentro do user namespace (em vez de uma cópia por container — medido: `containers/` de 47 para 7,2 GiB). Isso é matéria de [Anatomia do delonix](anatomia-delonix.html); aqui o que importa é que **o contrato com o exterior é este**: imagem OCI dentro, bundle OCI para o runtime.
