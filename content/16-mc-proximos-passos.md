---
title: "7 · Próximos passos"
slug: mc-proximos-passos
summary: Exercícios para estender o minicontainer — seccomp, rede, exec, vários uids e um servidor CRI — e onde o delonix faz cada um.
part: 4
order: 16
time: 15 min de leitura
---

# 7 · Próximos passos

O `minicontainer` está deliberadamente incompleto: cada coisa que ele **recusa** é um exercício, e cada uma tem uma implementação real no delonix para comparares. Ordenados do mais acessível ao mais ambicioso.

## Exercício 1 — `process.user` e `process.capabilities` (fácil)

O `config.json` já diz com que uid/gid e com que capabilities o processo deve correr; o `mc` ignora `process.user` (fica uid 0 no namespace) e usa as 14 fixas. **Implementa** `process.capabilities.bounding` (a lista do config em vez de `KEEP_CAPS`) e `process.user.uid/gid` com `setresuid`. Cuidado com a ordem: o `setuid` tem de vir **antes** de largares capabilities, com `PR_SET_KEEPCAPS`.

*No delonix:* o `setuid` corre antes do `drop_capabilities`, com `PR_SET_KEEPCAPS` (a ordem do runc) — foi o que fez um CoreDNS (uid 65532, `drop: ALL`) arrancar.

## Exercício 2 — `mc exec` (médio)

Correr um comando **dentro** de um container já a correr. Abre `/proc/<pid>/ns/{user,mnt,pid,uts,ipc,net}` e faz `setns` para cada, **user primeiro**; depois `chroot`/`fchdir` para a raiz do container e `fork` (o `pid` só vale para os filhos!). Repara na armadilha que o delonix pagou: **não** decidas se entras no user namespace por «o container criou o seu» — um container pode *herdar* um userns do holder, e sem o `setns(user)` o `unshare(NEWNS)` seguinte dá `EPERM`.

## Exercício 3 — seccomp (médio/difícil)

Instalar um filtro BPF com uma lista de syscalls permitidas. Duas escolhas: o crate `libseccomp` (liga a uma biblioteca C) ou escrever o programa BPF à mão. Começa por um filtro **negativo** (bloqueia `mount`, `reboot`, `kexec_load`, `init_module`…) e só depois a lista branca. Não te esqueças de `PR_SET_NO_NEW_PRIVS` (já lá está) e — ponto avançado — de fazer `clone3` devolver `ENOSYS`: senão contorna o filtro dos *flags* de `clone`. Depois **remove** a recusa de `linux.seccomp` no `validate` e testa com um bundle que a peça.

## Exercício 4 — rede: um veth e uma bridge (difícil)

Hoje o container só tem `lo`. Para falar com o mundo sem root precisas de um dos dois: `slirp4netns` (rede em userspace, o que o delonix usa para saída) ou um par **veth** com uma ponta num namespace de rede que **mantém a bridge** (o *holder* do delonix). Este é o exercício que mais mostra **porque** o delonix tem 18 000 linhas de rede: bridge, IPAM, DNS, NAT e `nftables`, tudo dentro de um netns que sobrevive ao plano de controlo. Começa por `slirp4netns` — é uma dúzia de linhas para o *attach*.

## Exercício 5 — vários uids com `newuidmap` (médio)

O mapeamento de um só uid não corre imagens com ficheiros de vários donos. Lê `/etc/subuid`/`/etc/subgid`, e em vez de escrever `/proc/<pid>/uid_map` directamente, chama o *helper* setuid `newuidmap`/`newgidmap` **a partir do pai** (só o pai, fora do namespace, tem o privilégio). Isto obriga a um *handshake* de dois passos: o filho faz `unshare(NEWUSER)`, avisa o pai por um pipe, o pai escreve os mapas, e devolve «ok».

## Exercício 6 — um servidor CRI (o grande)

O `minicontainer` cumpre a metade difícil do CRI: o ciclo de vida de containers. Para o kubelet o usar precisas de um servidor gRPC em `runtime.v1`. O esqueleto:

```toml
# Cargo.toml de um novo crate `mc-cri`
[dependencies]
minicontainer = { path = "../minicontainer" }
tonic = "0.12"   # versões indicativas — usa as actuais
prost = "0.13"
tokio = { version = "1", features = ["rt-multi-thread", "net"] }
tokio-stream = { version = "0.1", features = ["net"] }
[build-dependencies]
tonic-build = "0.12"     # gera os stubs a partir de api.proto do cri-api
```

```rust
// Esboço — NÃO faz parte do repositório nem foi validado.
#[tonic::async_trait]
impl RuntimeService for McRuntime {
    async fn version(&self, _: Request<VersionRequest>) -> Result<Response<VersionResponse>, Status> {
        Ok(Response::new(VersionResponse {
            version: "0.1.0".into(), runtime_name: "minicontainer".into(),
            runtime_version: env!("CARGO_PKG_VERSION").into(), runtime_api_version: "v1".into(),
        }))
    }
    async fn create_container(&self, req: Request<CreateContainerRequest>) -> Result<Response<CreateContainerResponse>, Status> {
        // 1. gerar o bundle a partir de req.config (imagem, args, envs, mounts, linux.resources)
        // 2. tokio::task::spawn_blocking(|| container::create(&store, &id, &bundle, Stdio::Log))
        // 3. mapear Error → tonic::Status: NotFound → NotFound, Conflict → AlreadyExists …
        todo!()
    }
    // start_container → container::start · stop_container → kill + wait_stopped · remove → delete
    // container_status → state + reconcile · list_containers → Store::list
}
```

Dois cuidados que vêm directamente do que já aprendeste:

- **`spawn_blocking`** para tudo o que chama `container::*` — faz `fork`, e um `fork` dentro de um *runtime* multi-thread é **exactamente** o bug do `clone()` bloqueado que o delonix pagou. Melhor ainda: re-exec do binário `mc` por pedido (é o que o delonix faz).
- **`RuntimeConfig` com o cgroup driver** que **realmente** usas (`cgroupfs` — escreves ficheiros, não pedes units ao systemd). Responder `linux: None` fez o kubelet do delonix matar pods.

Depois valida com o **cliente oficial** (`crictl`), na ordem: `version` → `info` → `runp` → `create` → `start` → `exec`.

## Exercício 7 — fuzzing (fácil, e útil)

`cargo install cargo-fuzz`, e escreve um alvo que alimenta `Spec` (`serde_json::from_slice`) e outro que alimenta `image::apply_layer` com um `tar` arbitrário. As duas são **parsers de input externo** — a superfície natural de um runtime.

## Comparar com o delonix

Cada exercício tem o seu par no motor, nesta ordem de crates:

| Exercício | Onde ler no delonix |
|---|---|
| 1 · `process.user`, caps | `delonix-linux` (`container_init`, `drop_capabilities`) |
| 2 · `exec` | `delonix-linux` (`runtime::exec`, `open_container_ns`) |
| 3 · seccomp | `delonix-linux` (o *allowlist* embutido) |
| 4 · rede | `delonix-sdn` (o *holder*, `infra.rs`) |
| 5 · vários uids | `delonix-linux` (o mapa com subuid, 3 caminhos de mapeamento) |
| 6 · CRI | `delonix-cri` (`runtime_svc.rs`) |

Já sabes ler cada um: o que muda é a escala. Quando estiveres pronto para contribuir, [o próximo capítulo](contribuir.html) diz como.
