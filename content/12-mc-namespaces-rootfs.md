---
title: "3 · Namespaces e rootfs"
slug: mc-namespaces-rootfs
summary: unshare, mapeamento de ids, mounts seguros, /dev sem mknod e pivot_root — o coração do isolamento, linha a linha.
part: 4
order: 12
time: 30 min de leitura
---

# 3 · Namespaces e rootfs

Este é o capítulo que transforma um processo normal num container. Segue a ordem em que as coisas acontecem — e a ordem é o conteúdo: cada passo depende do anterior.

{{svg:mc-processos}}

## Passo 1 — o supervisor entra nos namespaces

O supervisor faz um único `unshare` com cinco *flags*:

```rust
unshare(
    CloneFlags::CLONE_NEWUSER
        | CloneFlags::CLONE_NEWPID
        | CloneFlags::CLONE_NEWUTS
        | CloneFlags::CLONE_NEWIPC
        | CloneFlags::CLONE_NEWNET,
)?;
```

O `mnt` fica de fora **de propósito**: quem faz o `unshare(NEWNS)` é o *init*, mais tarde. Assim o supervisor mantém a **vista do host** do sistema de ficheiros, e pode gravar o `state.json` e apagar o cgroup depois de o container morrer.

E o `pid`? Lembra-te: `CLONE_NEWPID` só vale para os **filhos**. Por isso a seguir vem um `fork` — o filho é o PID 1.

## Passo 2 — mapear os ids (e a armadilha do `getuid`)

Um user namespace acabado de criar não tem mapa: és «ninguém» (uid 65534). Sem mapa, nem sequer podes escrever num ficheiro. O processo escreve o seu próprio mapa em `/proc/self`:

{{file:minicontainer/src/container.rs#map-ids}}

Três detalhes, todos aprendidos a falhar:

1. **`setgroups` = `deny` primeiro.** Sem privilégio, o kernel só deixa escrever o `gid_map` se se prometer nunca usar `setgroups(2)` (senão um utilizador poderia largar grupos que o restringem).
2. **Mapeia um só uid:** `0 <uid> 1` — «o uid 0 dentro é o meu uid fora, num intervalo de 1».
3. **Os ids são parâmetros**, lidos **antes** do `unshare`. O primeiro código chamava `getuid()` *dentro* da função, e depois do `unshare` isso devolve 65534 → o kernel recusa o mapa com `EPERM`, sem dizer porquê. Passar `uid`/`gid` como argumentos torna a ordem impossível de errar:

```rust
// Ler os ids ANTES do unshare: depois dele `getuid()` devolve o uid «overflow» (65534)
// e o `uid_map` seria recusado com EPERM.
let (uid, gid) = (nix::unistd::getuid(), nix::unistd::getgid());
```

## Passo 3 — o `lo` do namespace de rede

Um `net` namespace novo nasce **só com o `lo`, em baixo**. Sem o subir, `127.0.0.1` não responde. Uma das poucas `unsafe` do projecto — um `ioctl` que o `nix` não embrulha:

{{file:minicontainer/src/container.rs#loopback}}

Repara no que se faz **antes** de fechar o socket: guarda-se o `errno` (`nix::Error::last()`) *antes* do `close`, porque o `close` pode sobrescrevê-lo.

## Passo 4 — o init monta o rootfs

Agora, já como PID 1 num user namespace onde o processo tem *todas* as capabilities (sobre o namespace), o init prepara o seu mundo:

{{file:minicontainer/src/container.rs#init}}

Vamos por partes.

### `MS_PRIVATE` em toda a árvore

```rust
mount(None::<&str>, "/", None::<&str>, MsFlags::MS_REC | MsFlags::MS_PRIVATE, None::<&str>)?;
```

Sem isto, os *mounts* que fazemos **propagariam para o host** (a *propagation* por omissão em muitas distros é `shared`). É a primeira linha depois do `unshare(NEWNS)` em qualquer runtime, e a mais fácil de esquecer.

### O rootfs como ponto de montagem

O `pivot_root` exige que a nova raiz seja um **ponto de montagem**. Uma pasta comum não é — fazemos um *bind mount* dela sobre si própria.

### Mounts do `config.json`, sem sair do rootfs

{{file:minicontainer/src/container.rs#apply-mount}}

O destino de cada *mount* vem de um ficheiro que **não controlamos**. Se fosse `rootfs.join(destino)`, uma imagem com um symlink `etc -> /` faria um *bind mount* para `etc/passwd` acabar em `/etc/passwd` **do host**. Por isso passa por:

### Resolver caminhos sem sair do rootfs

{{file:minicontainer/src/fsutil.rs#resolve}}

Percorre o caminho **componente a componente**, e recusa `..`, prefixos e — o essencial — **qualquer symlink**. Tem duas variantes (`create`: cria os directórios em falta). O teste planta o symlink hostil e afirma a recusa:

```rust
std::os::unix::fs::symlink("/", dir.path().join("etc")).unwrap();
assert!(matches!(
    resolve_in_root(dir.path(), Path::new("/etc/passwd"), true),
    Err(Error::UnsafePath(_))
));
```

O motor tem o equivalente, e o comentário dele conta porquê:

{{snippet:safe-bind-target}}

!!! danger "`bind_volume` corre ANTES do `pivot_root`"
    Enquanto o `/` ainda é o real, `create_dir_all` e `open` **seguem symlinks**. Um symlink absoluto plantado pela imagem redirecciona o destino para um caminho arbitrário do **host**, e o motor cria ali directórios e ficheiros com o seu próprio uid. Não é hipotético: foi um achado de auditoria.

### `/dev` sem `mknod`

Um container precisa de `/dev/null`, `/dev/zero`… mas criar *device nodes* exige `CAP_MKNOD` **no namespace inicial**, que não temos. A solução do runtime rootless: um `tmpfs` em `/dev` (vem do `config.json`) e, por cima, *bind mounts* dos dispositivos **do host**:

{{file:minicontainer/src/container.rs#setup-dev}}

### O truque do `pivot_root(".", ".")`

```rust
chdir(rootfs)?;
pivot_root(".", ".")?;               // raiz nova e antiga empilhadas no mesmo sítio…
umount2(".", MntFlags::MNT_DETACH)?; // …e a antiga desmonta-se
chdir("/")?;
```

A forma «manual» exigia criar um directório `.old` dentro do rootfs para pendurar a raiz antiga e depois apagá-lo — um passo que **escreve no rootfs**, que pode ser só de leitura. Com `pivot_root(".", ".")` a raiz antiga fica *empilhada por baixo* da nova, e o `MNT_DETACH` desfaz-se dela. Não sobra nada por onde fugir.

### Rootfs só de leitura, e as flags «trancadas»

```rust
if spec.root.readonly { remount_ro("/")?; }
```

Um *remount* em user namespace tem uma regra subtil: as *flags* (`nosuid`, `nodev`, `noexec`) herdadas do mount do host estão **trancadas** — um remount que as retire dá `EPERM`. Por isso o `remount_ro` lê-as (`statvfs`) e volta a pô-las:

```rust
fn remount_ro(path: &str) -> Result<()> {
    let flags = MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY | inherited_flags(path);
    mount(None::<&str>, path, None::<&str>, flags, None::<&str>)?;
    Ok(())
}
```

O resultado, visto de dentro:

{{out:05-readonly}}

`/x` (raiz) recusa a escrita; `/tmp` (um `tmpfs`) aceita. É a diferença entre um rootfs imutável e um sítio para trabalhar.

## Verifica

```bash
cargo test -p minicontainer --test e2e runs_as_pid_1
cargo test -p minicontainer --test e2e readonly_root
```

Já temos um container isolado e com uma raiz própria. Falta limitá-lo e tirar-lhe poder: [cgroups e capabilities](mc-cgroups-caps.html).
