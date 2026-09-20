---
title: minicontainer — visão geral
slug: mc-visao
summary: O que vamos construir, o que fica de fora de propósito, e o mapa de processos — um runtime OCI rootless e sem daemon.
part: 4
order: 9
time: 15 min de leitura
---

# minicontainer — visão geral

Chegou a hora de juntar tudo. O **`minicontainer`** (comando `mc`) é um runtime OCI **rootless e sem daemon**, em Rust, que:

- lê um **bundle OCI** (`config.json` + `rootfs/`) e cumpre o **ciclo de vida** `create → start → kill → delete`, mais `state`, `list`, `logs` e `run`;
- **desempacota uma imagem OCI** (*image layout*) num bundle, verificando digests e aplicando *whiteouts*;
- isola com **user, mount, pid, uts, ipc e net namespaces**, faz `pivot_root`, limita com **cgroups v2**, reduz as **capabilities** ao conjunto OCI e liga `no_new_privs`;
- **recusa** o que não implementa, em vez de ignorar;
- passa em **22 testes** e corre o **mesmo bundle no `runc`**.

Está em [`minicontainer/`](https://github.com/angolardevops/delonix-rust-tutorial/tree/main/minicontainer). Menos de 1 800 linhas, testes incluídos — pequeno o bastante para o leres todo, real o bastante para correr containers a sério. Tudo o que vês nestes capítulos foi **corrido** neste host; as saídas vêm de [`scripts/demo.sh`](https://github.com/angolardevops/delonix-rust-tutorial/blob/main/scripts/demo.sh).

## Primeiro contacto

```bash
cargo build --release -p minicontainer
scripts/make-rootfs.sh /tmp/b            # bundle a partir do busybox estático do host
target/release/mc spec /tmp/b -- /bin/sh -c 'echo pid=$$ host=$(hostname) uid=$(id -u)'
target/release/mc run demo -b /tmp/b
```

{{out:01-run-basico}}

Um processo com **PID 1**, hostname próprio, `uid=0` (root *dentro* — tu, fora), `CapBnd` reduzido ao conjunto OCI (`a80425fb`, 14 capabilities) e um `/dev` mínimo. Sem `sudo`.

## O mapa de processos

O ponto de design mais importante, e o que mais se parece com o delonix: **quem é o «processo do container»?** Sem daemon, ninguém a correr em permanência — mas alguém tem de ver o container morrer e gravar o código de saída. A resposta: **um supervisor por container**, com dono claro.

{{svg:mc-processos}}

1. `mc create` faz `fork` → o **supervisor** (faz `setsid`, fica a correr sem terminal).
2. O supervisor faz `unshare` de **user, pid, uts, ipc, net**, mapeia os ids e sobe o `lo`; depois faz `fork` → o **init**.
3. O **init** (PID 1 do container) faz `unshare(mnt)`, monta o rootfs, faz `pivot_root`, reduz privilégios — e **bloqueia** a ler um FIFO. Só então diz «pronto» ao supervisor (por um *pipe*), que responde ao `mc create`.
4. `mc start` escreve **um byte** no FIFO. O init acorda e faz `exec` do processo do utilizador.
5. Quando o processo sai, o supervisor faz `waitpid`, grava `stopped` + código de saída **no estado em disco**, remove o cgroup e termina.

Toda a comunicação entre invocações do `mc` passa por **ficheiros** (`state.json`, o FIFO) — não há socket nem processo residente. É a mesma escolha do delonix: *«O que precisa de persistir é do systemd ou de um processo por workload com dono claro»*.

## Âmbito: o que faz e o que recusa

| Feature | `minicontainer` | delonix-runtime |
|---|---|---|
| Namespaces user/mnt/pid/uts/ipc/net | ✅ sempre isolados | ✅ |
| Rootless (user ns, 1 uid mapeado) | ✅ | ✅ (+ subuid/`newuidmap`) |
| `pivot_root`, rootfs só de leitura | ✅ | ✅ |
| Mounts `proc`/`tmpfs`/*bind* | ✅ | ✅ + volumes, NFS/CIFS |
| Limites cgroup v2 (mem/pids/cpu) | ✅ (recusa se sem delegação) | ✅ + cpuset, io, OOM ao vivo |
| Capabilities OCI + `no_new_privs` | ✅ | ✅ + CDI, ambient |
| Imagem OCI (layout local, digests, whiteouts) | ✅ | ✅ + pull de registos, CAS, overlay partilhado |
| Estado em disco com `flock` | ✅ | ✅ |
| **seccomp** | ❌ **recusado** | ✅ |
| **pty / `terminal: true`** | ❌ **recusado** | ✅ |
| **Juntar-se a namespaces existentes** | ❌ **recusado** | ✅ (pods) |
| Rede (veth, bridge, NAT, DNS) | ❌ só `lo` | ✅ SDN completa |
| `exec` num container a correr | ❌ | ✅ |
| Vários uids (`newuidmap`) | ❌ 1 uid | ✅ |
| CRI (servidor gRPC) | ❌ ([exercício](mc-proximos-passos.html)) | ✅ |

!!! delonix "O que a coluna da direita ensina"
    Um runtime real é *dezenas* de coisas por cima deste núcleo. O valor do `minicontainer` é ser **o núcleo**, legível de uma ponta à outra: quando fores ler `delonix-linux`, vais reconhecer cada passo — só que com quinze mil linhas de casos difíceis à volta.

## Como ler os capítulos seguintes

| Capítulo | Módulo | Conceito |
|---|---|---|
| [10 · Spec e erros](mc-spec-e-erros.html) | `spec.rs`, `error.rs` | recusar em vez de ignorar; erros com classe |
| [11 · Imagem OCI](mc-imagem-oci.html) | `image.rs` | digests, layers, whiteouts |
| [12 · Namespaces e rootfs](mc-namespaces-rootfs.html) | `container.rs`, `fsutil.rs` | `unshare`, mapa de ids, `pivot_root` |
| [13 · cgroups e capabilities](mc-cgroups-caps.html) | `cgroup.rs`, `container.rs` | limites, OOM, privilégios |
| [14 · Ciclo de vida e estado](mc-ciclo-de-vida.html) | `state.rs`, `container.rs` | supervisor, FIFO, `flock` |
| [15 · Testes e validação](mc-testes.html) | `tests/`, `runc` | provar, e os bugs que apanhámos |
| [16 · Próximos passos](mc-proximos-passos.html) | — | seccomp, rede, CRI |

O `Cargo.toml` do crate:

{{file:minicontainer/Cargo.toml}}

Repara na lista curta de dependências: `nix` (chamadas de sistema com tipos seguros), `serde` (a spec), `sha2`+`hex` (digests), `flate2`+`tar` (layers), `thiserror`, `clap`, e `libc` para as duas ou três chamadas que o `nix` não cobre. **Nenhum runtime assíncrono**: não há I/O concorrente aqui.
