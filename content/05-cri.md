---
title: CRI e o Kubernetes
slug: cri
summary: Como o kubelet fala com um runtime — os serviços gRPC do CRI, o conceito de sandbox, e o mapeamento CRI → OCI.
part: 2
order: 5
time: 25 min de leitura
---

# CRI e o Kubernetes

O OCI diz como correr **um container**. O Kubernetes precisa de outra coisa: gerir **pods** (grupos de containers que partilham rede), puxar imagens, dar `exec` e `logs`, reportar estado, e fazê-lo para **qualquer** runtime. É o papel do **CRI — Container Runtime Interface**.

{{svg:cri-pilha}}

O CRI é uma API **gRPC** (`runtime.v1`) que o **kubelet** chama por um socket unix. Do outro lado está um runtime «de alto nível» (containerd, CRI-O — ou o `delonix-cri`) que, por baixo, usa um runtime OCI «de baixo nível» (`runc`).

!!! rust "Onde o delonix se encaixa"
    O delonix **é os dois níveis** num só binário, sem daemon central: o `delonix-cri` implementa o serviço CRI e cria os containers com o próprio motor (não delega em `runc`). `delonix serve cri` limita-se a fazer `exec` desse binário. O kubelet aponta-lhe com `--container-runtime-endpoint`.

## Os dois serviços

| `RuntimeService` | `ImageService` |
|---|---|
| `RunPodSandbox`, `StopPodSandbox`, `RemovePodSandbox` | `PullImage` |
| `PodSandboxStatus`, `ListPodSandbox` | `ListImages`, `ImageStatus` |
| `CreateContainer`, `StartContainer`, `StopContainer`, `RemoveContainer` | `RemoveImage` |
| `ContainerStatus`, `ListContainers`, `ContainerStats` | `ImageFsInfo` |
| `ExecSync`, `Exec`, `Attach`, `PortForward` | |
| `UpdateContainerResources`, `Version`, `Status` | |

## O conceito que não existe no OCI: o *sandbox*

Um **pod** partilha rede (e IPC, e opcionalmente PID). O CRI modela-o como um **sandbox**: um ambiente vazio, criado **primeiro**, ao qual se juntam containers depois. Em runtimes Linux clássicos o sandbox é um processo «pause» que segura os namespaces; o delonix guarda a netns do pod num *holder*.

Sequência típica quando o kubelet cria um pod com dois containers:

```text
RunPodSandbox(config)              → devolve pod_sandbox_id   (rede, cgroup pai do pod)
PullImage(nginx) ; PullImage(sidecar)
CreateContainer(pod_sandbox_id, nginx)    → container_id_1
CreateContainer(pod_sandbox_id, sidecar)  → container_id_2
StartContainer(container_id_1) ; StartContainer(container_id_2)
   ... vida do pod: ContainerStatus / ListContainers / ContainerStats em ciclo ...
StopContainer(...)  ; RemoveContainer(...)
StopPodSandbox(id)  ; RemovePodSandbox(id)
```

## O mapeamento CRI → OCI

Repara como cada chamada de container se traduz **directamente** nas operações do ciclo de vida OCI do [capítulo anterior](oci.html). É por isto que um runtime OCI é *reutilizável* debaixo de um CRI:

| CRI (`RuntimeService`) | Operação OCI | No `minicontainer` |
|---|---|---|
| `CreateContainer` | `create` (+ gerar o `config.json`) | `mc create` |
| `StartContainer` | `start` | `mc start` |
| `StopContainer` (com *grace period*) | `kill SIGTERM`, espera, `kill SIGKILL` | `mc kill` |
| `RemoveContainer` | `delete` | `mc delete` |
| `ContainerStatus` | `state` | `mc state` |
| `ListContainers` | listar `state` de todos | `mc list` |
| `RunPodSandbox` | criar o sandbox (holder de namespaces + rede) | *(fora do âmbito)* |
| `ExecSync`/`Exec` | `exec` num container já a correr (`setns`) | *(fora do âmbito)* |

O que **falta** ao `minicontainer` para ser um CRI é o **servidor gRPC** e o sandbox — está proposto como [exercício final](mc-proximos-passos.html). O ciclo de vida que ele já cumpre é a metade difícil.

## Lições reais de um CRI em produção

O `delonix-cri` foi validado contra um **kubelet real** (k8s 1.36, `kubeadm init`), e o que apareceu é o que não se lê em nenhuma especificação:

### 1. O `cgroupDriver` que o runtime declara manda no kubelet

O kubelet lê o driver de cgroups **do runtime** (`RuntimeConfig`) e ignora o da sua própria config. Quando o `delonix-cri` respondia `linux: None`, o kubelet voltava ao default do kubeadm — `systemd` — e criava os cgroups dos pods como *slices* do systemd **vazios** (os containers viviam noutro sítio). O systemd retira dos slices vazios um controlador que ninguém usa: o `cpuset`. O kubelet, ao validar o cgroup do pod, via-o sem `cpuset` e **matava o pod**, sem uma linha de log própria. O `control-plane` entrava em *crash-loop*.

{{snippet:cri-cgroup}}

A correcção são três linhas — mas a **causa** só apareceu com um vigia de 100 ms sobre `cgroup.controllers` e um `systemctl daemon-reload` a fazer o `cpuset` desaparecer *à vontade*. Mesmo nó reposto, só o driver mudou: `/livez` **84/84** durante 7 minutos e 0 paragens, contra 20 paragens em 3 minutos.

!!! tip "O método, mais valioso que o remédio"
    Isolar a causa por **intervenção controlada** (mudar *uma* variável no mesmo nó reposto), não por leitura de código. Está em [Projecto completo](projecto-completo.html) como regra de qualidade.

### 2. Um limite pedido e ignorado é pior que um limite que não existe

O `linux.resources` do `CreateContainer` **não era lido por ninguém**: os `resources.limits` de um pod nunca chegavam ao container, e o scheduler julgava-o limitado. Um OOM só passou a acontecer (`OOMKilled`, código 137) depois de o CRI traduzir os limites — e só se detecta a morte por OOM **ao vivo**, porque o cgroup desaparece com o container: lê-se `memory.events` *antes* do `waitpid`. O `minicontainer` faz exactamente isso — ver o [`oom_killed`](mc-cgroups-caps.html) do estado.

### 3. O `stderr` do runtime tem de chegar ao kubelet

Um `StartContainer` falhado dizia ao kubelet só «failed to start container <id>». A razão do motor (um `EPERM` a montar) ficava num `stderr` descartado. Escreve a razão para um **ficheiro**, nunca para um pipe que ninguém lê: um `run -d` segura os descritores que herdou.

## Validar com o cliente oficial: `crictl`

Não confies num servidor CRI que só testaste com o teu cliente. O `crictl` (do projecto Kubernetes) é o cliente de referência:

```bash
export CONTAINER_RUNTIME_ENDPOINT=unix:///run/delonix-cri.sock
crictl version          # RuntimeName: delonix, RuntimeApiVersion: v1
crictl info             # RuntimeReady: true
crictl runp pod.json    # sandbox Ready
crictl create <pod> c.json pod.json && crictl start <ctr>
crictl exec -it <ctr> sh
```

O delonix corre esta sequência (`version` → `info` → `runp` → `create` → `start` → `exec`) e labels/annotations do kubelet sobrevivem ao *round-trip*. Continua por validar com **vários nós** — está dito na documentação do motor, e é o tipo de coisa que deves dizer também no teu.
