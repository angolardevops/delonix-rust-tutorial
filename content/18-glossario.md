---
title: Glossário e recursos
slug: glossario
summary: Termos de containers, Linux e Rust usados neste tutorial, e as referências canónicas para ir mais longe.
part: 5
order: 18
time: consulta
---

# Glossário e recursos

## Containers e Linux

| Termo | Significado |
|---|---|
| **Bundle** | Pasta OCI com `config.json` e `rootfs/` — o que um runtime de baixo nível corre. |
| **Bounding set** | O tecto de capabilities que qualquer descendente de um processo pode ter. |
| **cgroup v2** | Hierarquia única em `/sys/fs/cgroup` para limitar recursos; escreve-se em ficheiros (`memory.max`). |
| **CRI** | *Container Runtime Interface*: API gRPC que o kubelet usa para falar com um runtime. |
| **Delegação (cgroup)** | Entregar uma subárvore de cgroups a um utilizador não-root (`Delegate=yes` no systemd). |
| **Digest** | Hash de conteúdo (`sha256:…`) que identifica um blob OCI. |
| **Holder** | (delonix) processo mínimo que segura um netns de vida longa para o plano de controlo poder reiniciar. |
| **Image layout** | Formato em disco de uma imagem OCI: `index.json`, `oci-layout`, `blobs/sha256/`. |
| **Init (PID 1)** | O primeiro processo do PID namespace; só recebe sinais que tenha tratado (excepto `SIGKILL` de fora). |
| **Layer** | Um *diff* de sistema de ficheiros (tar) numa imagem; empilham-se por ordem. |
| **Namespace** | Vista isolada de um recurso do kernel (pid, mnt, net, uts, ipc, user, cgroup, time). |
| **OCI** | *Open Container Initiative*: image-spec e runtime-spec. |
| **`pivot_root`** | Troca a raiz do mount namespace; ao contrário do `chroot`, permite desmontar a antiga. |
| **Rootless** | Correr sem privilégios de root no host, com o poder confinado a um user namespace. |
| **Sandbox (CRI)** | Ambiente de um pod (rede, cgroup pai) ao qual se juntam containers. |
| **Seccomp** | Filtro BPF sobre as syscalls que um processo pode fazer. |
| **Supervisor** | Processo por container que espera a sua morte e regista o resultado (sem daemon central). |
| **User namespace** | Mapeia uids/gids; permite ser «root» só sobre o namespace. |
| **Whiteout** | Ficheiro `.wh.<nome>` numa layer que significa «apaga `<nome>`». |

## Rust e engenharia

| Termo | Significado |
|---|---|
| **Newtype** | Um tipo com um só campo para carregar uma garantia (`ContainerId`). |
| **Typestate** | Estado codificado em parâmetro de tipo; transições ilegais não compilam. |
| **Ratchet** | Métrica que falha se sobe **e** se desce sem a base baixar — a melhoria fica registada. |
| **Fail-closed** | Recusar quando não se consegue cumprir, em vez de seguir sem a garantia. |
| **Idempotente** | Aplicar N vezes = aplicar uma; a prova é «zero escritas» a partir da segunda. |
| **Reconciliação (3 vias)** | Comparar desejado, actual **e** último aplicado, para distinguir remoção de edição alheia. |
| **ADR** | *Architecture Decision Record*: uma decisão de fronteira, com contexto e consequências. |
| **MSRV** | *Minimum Supported Rust Version*, declarado em `rust-version`. |
| **`// SAFETY:`** | Comentário obrigatório antes de um `unsafe`, a justificar as pré-condições. |

## Referências canónicas

**Normas**

- [OCI Runtime Specification](https://github.com/opencontainers/runtime-spec) e [Image Specification](https://github.com/opencontainers/image-spec)
- [CRI API (`k8s.io/cri-api`)](https://github.com/kubernetes/cri-api) · [`crictl`](https://github.com/kubernetes-sigs/cri-tools)
- [`runc`](https://github.com/opencontainers/runc) — o runtime de referência; ler `libcontainer/` é um dos melhores exercícios

**Linux** (as páginas do `man` são o texto definitivo)

`namespaces(7)` · `user_namespaces(7)` · `pid_namespaces(7)` · `mount_namespaces(7)` · `cgroups(7)` · `capabilities(7)` · `pivot_root(2)` · `unshare(2)` · `setns(2)` · `seccomp(2)`. E o [`Documentation/admin-guide/cgroup-v2.rst`](https://docs.kernel.org/admin-guide/cgroup-v2.html) do kernel.

**Rust**

- *The Rust Programming Language* (o «livro») · *Rust by Example* · *The Rustonomicon* (`unsafe`) · *Rust API Guidelines*
- *Rust Design Patterns* · *Effective Rust*
- Ferramentas: [`clippy`](https://github.com/rust-lang/rust-clippy), [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny), [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz), [`cargo-semver-checks`](https://github.com/obi1kenobi/cargo-semver-checks)

**Contêineres e sistemas em Rust, para ler**

- [`youki`](https://github.com/containers/youki) — outro runtime OCI em Rust
- [`nix`](https://github.com/nix-rust/nix) — a camada de chamadas de sistema usada aqui
- O próprio [`delonix-runtime`](https://github.com/angolardevops/delonix-runtime) e o seu manual `docs/dev/`

## Atalhos deste site

| | |
|---|---|
| <kbd>Ctrl</kbd> + <kbd>K</kbd> ou <kbd>/</kbd> | Pesquisar em todo o tutorial |
| <kbd>↑</kbd> <kbd>↓</kbd> <kbd>↵</kbd> | Navegar e abrir resultados |
| <kbd>Esc</kbd> | Fechar a pesquisa |
| ◐ (canto superior) | Tema: automático → claro → escuro |
