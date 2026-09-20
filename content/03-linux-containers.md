---
title: O que é um container
slug: linux-containers
summary: Namespaces, cgroups v2, capabilities, pivot_root e rootless — com saídas reais medidas neste host.
part: 2
order: 3
time: 35 min de leitura
---

# O que é um container

> Um container **não existe** no kernel Linux. Não há uma chamada `create_container()`. Existe um **processo normal** a que se aplicaram, em conjunto, mecanismos independentes de isolamento e de limitação. O «runtime» é o programa que os aplica pela ordem certa.

Guarda esta frase: todo o resto do tutorial é a sua consequência. Quatro mecanismos, cada um respondendo a uma pergunta diferente:

| Mecanismo | Pergunta | Exemplo |
|---|---|---|
| **Namespaces** | *O que é que o processo vê?* | o seu próprio `/`, PIDs, hostname, rede |
| **cgroups v2** | *Quanto é que pode gastar?* | memória, CPU, nº de processos |
| **Capabilities / seccomp** | *O que é que pode fazer?* | `chown`, montar, `mknod` — ou não |
| **`pivot_root`** | *Qual é o seu `/`?* | o rootfs da imagem, não o do host |

{{svg:container-anatomia}}

## Namespaces: o que o processo vê

Cada processo pertence a um namespace **de cada tipo**. Vê-los é ler `/proc/self/ns` — esta é a saída de um shell neste host:

{{out:20-namespaces-do-processo}}

Oito tipos (o `time` chegou no kernel 5.6):

| Namespace | Isola | Sintoma de o teres |
|---|---|---|
| `mnt` | pontos de montagem | o container tem o seu próprio `/` |
| `pid` | numeração de processos | o teu processo é o **PID 1** |
| `uts` | hostname e domínio | `hostname` diferente do host |
| `ipc` | filas/memória partilhada SysV | sem `ipcs` do host |
| `net` | interfaces, rotas, firewall | só um `lo`, em baixo |
| `user` | uids/gids | **root dentro, ninguém fora** |
| `cgroup` | vista da hierarquia de cgroups | `/sys/fs/cgroup` próprio |
| `time` | relógios monotónicos | raro |

Sem escrever uma linha de código, o `unshare(1)` mostra três deles a funcionar — user, pid e uts:

{{out:21-unshare}}

Repara: dentro, `pid=1` e `hostname=dentro`; **fora**, o hostname do host não mudou. O mesmo em Rust — com um ponto subtil que já enganou muita gente — está em [`examples/src/bin/ns_demo.rs`](https://github.com/angolardevops/delonix-rust-tutorial/blob/main/examples/src/bin/ns_demo.rs), e o teste de integração confirma **de fora** que o host ficou intacto.

!!! warning "Armadilha: o `pid` namespace só vale para os *filhos*"
    `unshare(CLONE_NEWPID)` **não** move o processo que chama — só os que ele criar depois. Por isso o código faz `unshare` **e depois** `fork`: o filho é o PID 1. Já o `uts` e o `user` aplicam-se logo a quem chama (e os filhos herdam), o que faz com que o «pai» do demo veja o hostname que o filho pôs: estão no mesmo namespace novo.

### User namespace: a chave do rootless

O `user` namespace é o que permite tudo o resto **sem ser root**. Um processo sem privilégio pode criar um user namespace novo e, **lá dentro**, ter todas as capabilities — sobre esse namespace apenas. O preço: os uids têm de ser **mapeados**. Sem mapa, és ninguém:

{{out:22-uid-map-overflow}}

Sem mapeamento vês `uid=65534(nobody)` (o uid de *overflow*). Com `--map-root-user`, o `uid_map` diz `0 1000 1`: «o uid 0 lá dentro **é** o uid 1000 cá fora, num intervalo de 1». Um mapeamento de um só uid é o que se consegue sem `newuidmap`/`/etc/subuid`; para vários uids (um container com utilizadores diferentes) precisas do *helper* setuid `newuidmap`.

!!! danger "Armadilha que custou um EPERM misterioso"
    Ao escrever o `minicontainer`, o `uid_map` falhava com «Operation not permitted» — mesmo escrevendo `0 <uid> 1`. Causa: eu chamava `getuid()` **depois** do `unshare`, e aí devolve o uid de *overflow* (65534). Lê os ids **antes** de entrares no namespace. Está comentado no código: [`map_ids`](mc-namespaces-rootfs.html).

## cgroups v2: quanto pode gastar

Os namespaces limitam o que se **vê**; não limitam o que se **gasta**. Um container sem cgroup pode comer toda a RAM do nó. Os *control groups* v2 formam **uma só hierarquia** em `/sys/fs/cgroup`:

{{out:23-cgroup-v2}}

Cada directório é um cgroup; ficheiros como `memory.max`, `pids.max`, `cpu.max` são os limites. Para limitar, **escreves num ficheiro** — não há API.

### O problema: o cgroup tem de ser *teu*

Numa sessão normal, não podes criar filhos com controladores activos: o scope da sessão pertence ao systemd. Só numa subárvore **delegada** ao teu utilizador é que o kernel deixa:

{{out:24-cgroup-delegado}}

O `Delegate=yes` fez o systemd entregar-nos um cgroup próprio, com `cpu memory pids` disponíveis. Note-se o que **não** aparece: `cpuset` e `io` — o `user.slice`, que é do root, não os passa para baixo. É o estado normal de um host rootless.

!!! delonix "Lição de produção: limites que não se aplicam"
    O delonix mediu isto numa VM limpa acedida por SSH: `-m 128M --cpus 0.5` ficavam **inertes** — `memory.max=max`. Não é bug: o scope de sessão SSH é *irmão* de `user@UID.service`, não filho. A resposta do motor (e do nosso `minicontainer`) é **recusar** um limite que não consegue aplicar, com o remédio na mensagem — em vez de fingir que o aplicou. Ver [cgroups no mini-projecto](mc-cgroups-caps.html).

Regra a saber, porque morde toda a gente: **«no internal processes»**. Um cgroup só pode activar controladores para os filhos (`cgroup.subtree_control`) se **não tiver processos directos**. Por isso o runtime move-se primeiro para um cgroup «gestor» (`mc-mgr`; o delonix chama-lhe `dlx-mgr`) e só depois activa `+memory +pids +cpu`.

## Capabilities: o que pode fazer

O root clássico é tudo-ou-nada. As **capabilities** dividem-no em ~40 poderes (`CAP_CHOWN`, `CAP_NET_ADMIN`, `CAP_SYS_ADMIN`…). Um container não precisa da maioria. O que importa é o **conjunto limitador** (*bounding set*): o tecto do que qualquer processo filho pode alguma vez ter.

{{out:25-capabilities}}

Repara na segunda medição: dentro de um user namespace criado por nós, `CapEff = 1ffffffffff` — **todas** as capabilities. São *sobre o namespace*, não sobre o host, mas continuam a ser demasiado poder para uma carga qualquer. A OCI define um conjunto por omissão de 14, e é o que o runtime deve deixar:

{{out:26-capsh-decode}}

`a80425fb` são exactamente essas 14. Vais ver o `minicontainer` reduzir o *bounding set* a este valor — e um teste a verificar `CapBnd == a80425fb`.

## `pivot_root`: o `/` do container

Um container tem o seu próprio sistema de ficheiros raiz (o *rootfs* da imagem). Duas formas de o trocar:

| | `chroot` | `pivot_root` |
|---|---|---|
| O que faz | muda a raiz **para um processo** | troca a raiz do **mount namespace** |
| O antigo `/` | continua acessível (há fugas conhecidas) | pode ser **desmontado** |
| Usa-se em | scripts, *jails* simples | **todos** os runtimes |

A sequência, que o `minicontainer` implementa em [`container_init`](mc-namespaces-rootfs.html):

1. `unshare(NEWNS)` e `mount(/, MS_REC|MS_PRIVATE)` — para os mounts **não propagarem** ao host;
2. *bind mount* do rootfs sobre si próprio (o `pivot_root` exige que a nova raiz seja um ponto de montagem);
3. `chdir(rootfs)`, `pivot_root(".", ".")` — a raiz nova e a antiga ficam empilhadas no mesmo sítio;
4. `umount2(".", MNT_DETACH)` — a antiga desaparece; `chdir("/")`.

## Seccomp: o que pode pedir ao kernel

O último eixo é o **filtro de syscalls** (seccomp-BPF): uma lista de chamadas permitidas, com o resto a devolver erro ou a matar o processo. O `minicontainer` **não** o implementa — e o `config.json` que peça `linux.seccomp` é **recusado** com erro claro, não ignorado. O delonix implementa-o e tem um *allowlist* embutido, mais uma regra que apanhámos a custo: instalar **sempre** o filtro que faz `clone3` devolver `ENOSYS`, porque um `clone3` deixaria contornar o filtro dos *flags* de `clone`.

!!! tip "Resumo: o que cada peça fecha"
    Sem namespaces vês tudo; sem cgroups gastas tudo; sem capabilities podes tudo; sem `pivot_root` o teu `/` é o do host. **Um container é a soma dos quatro** — e uma falha em qualquer um é uma fuga.

## Rootless-first

O delonix corre **sem root por omissão**. Não é um detalhe de conveniência, é uma decisão de segurança: se o motor for comprometido, o atacante tem os poderes de um utilizador normal, não os do sistema. O que isto custa (e o motor documenta): um só uid mapeado sem `newuidmap`, sem `mknod` (os `/dev/null`, `/dev/zero`… são *bind mounts* dos do host), portas < 1024 dependem de `ip_unprivileged_port_start`, e limites de cgroup exigem delegação.

## Próximo passo

Já sabes o que um runtime aplica. Falta saber **o quê** aplicar: qual é o formato da imagem e da configuração? É a norma OCI — [próximo capítulo](oci.html).
