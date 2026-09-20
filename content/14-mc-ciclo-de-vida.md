---
title: "5 · Ciclo de vida e estado"
slug: mc-ciclo-de-vida
summary: Supervisor por container, protocolo de arranque por pipe e FIFO, estado em disco com flock, e reconciliação com a realidade.
part: 4
order: 14
time: 30 min de leitura
---

# 5 · Ciclo de vida e estado

Como é que `mc create` volta ao terminal, deixando um container a esperar, e `mc start` — um **processo diferente**, minutos depois — o faz arrancar? Sem daemon, sem socket. A resposta é um **supervisor**, um **FIFO** e **ficheiros**.

## `create`: dois relatórios em cadeia

{{file:minicontainer/src/container.rs#spawn}}

O `create` faz `fork` e espera um **relatório** do supervisor por um pipe. O supervisor faz o mesmo ao seu filho (o init). É uma cadeia de dois elos, e cada elo **traduz o erro do seguinte**:

```text
mc create ──pipe──◀ supervisor ──pipe──◀ init
                       (relatório K<pid> ou E<mensagem>)
```

Se o init falhar a montar o rootfs, a mensagem sobe até ao utilizador (`mc: container setup failed: … EPERM …`) e o `create` **desfaz** o que criou (`store.remove(id)`) — nunca deixa um meio-container.

### O protocolo do relatório — e o bug do EOF

{{file:minicontainer/src/container.rs#recv}}

Uma leitura, **não** «até ao EOF». A primeira versão fazia `read_to_string` e o `mc create` **bloqueava para sempre**: o init mantém o pipe aberto até ao `exec` (só acontece depois do `start`), logo o EOF nunca chegava. O comentário no código guarda a lição. Bug real, apanhado ao correr a primeira vez.

## O supervisor

{{file:minicontainer/src/container.rs#supervise}}

Lê-o como uma história:

1. **`setsid`** — deixa de ter terminal de controlo; sobrevive ao fecho da shell que o lançou.
2. **`detach_stdio`** (só em `create`) — o stdio do container vai para `output.log`; o `stdin` para `/dev/null`.
3. `unshare` + `map_ids` + `loopback_up` ([capítulo 12](mc-namespaces-rootfs.html)), depois `fork` do **init**.
4. **Anexa o init ao cgroup** logo que existe — antes de o init fazer o resto do *setup*.
5. Espera o relatório do init e reencaminha-o.
6. **`waitpid(init)`** — e daqui em diante só há um trabalho: ver o container morrer e gravar o resultado.

### O bug do stdout que prende pipes

{{file:minicontainer/src/container.rs#stdio}}

Bug **real**, apanhado pelos testes de integração: `Command::output()` **bloqueou 3 minutos** num `mc create`. O supervisor e o init herdavam o `stdout` do chamador — um pipe —, e enquanto o container vivesse, esse pipe tinha um escritor, logo `output()` nunca via EOF. É o mesmo defeito que o delonix documenta com o processo do *pin* de rede: «*o processo é detached*» **não** quer dizer «já não preciso dos fds do chamador». A regra: um processo de vida longa **nunca** herda o stdio de quem o lançou. Em `create` vai para um ficheiro (`mc logs <id>`); em `run` (foreground) herda de propósito, para veres a saída.

## `start`: um byte num FIFO

{{file:minicontainer/src/container.rs#start}}

O init está bloqueado num `read` de 1 byte do FIFO. `start` **abre-o e escreve um byte**. É tudo. O FIFO foi criado com `mkfifo` no `create` e aberto **antes do `pivot_root`** (depois dele o caminho já não existe); aberto `O_RDWR`, para o `open` não bloquear.

Duas subtilezas:

- **Compare-and-set.** Um comando curto (`echo hi`) pode terminar **antes** de o `start` gravar `running`. Se o `start` escrevesse `running` sem olhar, sobrepunha o `stopped` que o supervisor já gravou. Por isso só passa a `running` **se ainda estiver `created`** — e isto só funciona porque as duas escritas passam pelo mesmo `flock`:

## Estado em disco: `flock` e escrita atómica

{{file:minicontainer/src/state.rs#state-update}}

O padrão de sempre — o do [capítulo 6](sistemas-distribuidos.html#flock-read-modify-write): **lock exclusivo → reler sob o lock → aplicar a closure → escrever atómico**. A closure recebe o estado **actual** e decide (`Ok(())` grava; `Err` aborta sem gravar). O teste que o guarda lança 16 *threads*:

{{file:minicontainer/src/state.rs#concurrent-test}}

Sem o `flock`, algumas das 16 escritas perdiam-se e `pid` acabava < 16.

O ficheiro de *lock* é um ficheiro **à parte** (`lock`), e não o `state.json`, porque o `state.json` é substituído por `rename` — um lock sobre um ficheiro que é apagado e recriado deixa de proteger nada.

## `reconcile`: o estado em disco pode mentir

O supervisor pode ser morto (`kill -9`), a máquina pode reiniciar. O `state.json` diz `running` para um PID que já não existe.

{{file:minicontainer/src/state.rs#reconcile}}

Nunca se confia só no ficheiro: confronta-se com `kill(pid, 0)` (o sinal 0 não envia nada, só testa se o processo existe). É a versão mínima do `reconcile_status` do delonix.

!!! warning "Limitação conhecida: PIDs reciclados"
    `kill(pid, 0)` responde «existe *algum* processo com este PID» — não «o *meu* container». Num host de vida longa o PID pode ser reciclado. O delonix guarda o `starttime` do processo (de `/proc/<pid>/stat`) e só sinaliza se bater (`safe_to_signal`) — um processo de outro nunca leva um `SIGTERM` por engano.

## O ciclo completo, com a saída real

{{out:04-lifecycle}}

Lê os detalhes, porque cada linha é uma decisão:

- `create` → `created` com o **PID real** do init (host); `start` → `running`; `logs` mostra o que o container escreveu.
- `create` duplicado → `already exists`, saída **5**. `delete` de um vivo → recusa, saída **5**.
- `kill` (SIGKILL) → `stopped`. Um `SIGTERM` **não** funcionaria: o init de um PID namespace só recebe sinais que tenha tratado — excepto `SIGKILL` vindo de fora, que é garantido. É a mesma razão pela qual `docker stop` espera e depois manda `SIGKILL`.
- Depois de `delete`, `state` responde `no such container`, saída **4**.

## Verifica

```bash
cargo test -p minicontainer state::
cargo test -p minicontainer --test e2e lifecycle
```

Falta responder à pergunta mais importante do projecto: **como sabemos que funciona?** [Testes e validação](mc-testes.html).
