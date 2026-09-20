---
title: "6 · Testes e validação"
slug: mc-testes
summary: 22 testes, o mesmo bundle no runc, e os seis bugs reais que só a execução revelou — com a causa de cada um.
part: 4
order: 15
time: 25 min de leitura
---

# 6 · Testes e validação

«Compila» não é «funciona», e «o comando devolveu 0» não é «fez o que devia». Este capítulo mostra **como** se prova um runtime — e sobretudo o que a prova encontrou que a leitura do código não encontrou.

## O resultado

{{out:30-cargo-test}}

`22` testes no `minicontainer` (15 unidade + 7 integração), mais os 16 + 1 + 3 dos exemplos. `clippy -D warnings`, `fmt` e `cargo deny` limpos. Isto é o que corre em CI a cada PR.

## Testes de integração: o binário a sério

Os 7 testes de `tests/e2e.rs` **correm o binário** (`env!("CARGO_BIN_EXE_mc")`) com o kernel real. O que cada um prova:

| Teste | Prova |
|---|---|
| `runs_as_pid_1_with_its_own_hostname` | `pid=1 host=minicontainer` — PID e UTS namespaces |
| `propagates_the_exit_code_of_the_workload` | `exit 7` → o `mc` sai com 7 |
| `a_missing_binary_exits_127_like_a_shell` | binário inexistente → 127, como uma shell |
| `drops_capabilities_to_the_oci_default_set` | `CapBnd == 00000000a80425fb` |
| `readonly_root_is_enforced_but_tmpfs_is_writable` | `root-ro` / `tmp-rw` |
| `lifecycle_…_with_stable_error_classes` | `create→start→kill→delete`, saídas **4** e **5** |
| `a_bundle_asking_for_seccomp_is_refused_not_ignored` | saída **2** e a palavra `seccomp` no erro |

O *setup* constrói um rootfs mínimo a partir do busybox do host, e **salta com aviso** se não há user namespaces:

{{file:minicontainer/tests/e2e.rs#e2e-setup}}

Ver [a checklist de projecto](projecto-completo.html): um teste que salta em silêncio lê-se como verde.

## A contra-prova: o mesmo bundle no `runc`

Um teste escrito por quem escreveu o código herda os mesmos pressupostos que o código. A prova de conformidade que **não** herda é outro implementador:

{{out:13-runc-mesmo-bundle}}

O bundle que o `mc unpack` gerou e o `mc run` correu foi entregue ao `runc` 1.5.1, com a mesma saída. (O `runc` exige os mapeamentos de user namespace explícitos no `config.json`; o `mc` *sempre* os cria — é a diferença de contrato, não de resultado.)

## Os bugs que só a execução encontrou

Seis, por ordem de descoberta. Cada um passou por «o código parece certo» e falhou quando foi corrido — e cada um está **fixado por um teste ou por um comentário** no código.

### 1. `uid_map: Operation not permitted`

O primeiro `mc run` falhou a escrever `/proc/self/uid_map`. O mesmo mapeamento feito à mão em Python funcionava. **Causa:** `getuid()` chamado *depois* do `unshare(NEWUSER)` devolve 65534 (o uid de *overflow*), logo o mapa `0 65534 1` é recusado. **Cura:** ler os ids antes e passá-los como argumentos ([passo 2 do capítulo 12](mc-namespaces-rootfs.html#passo-2-mapear-os-ids-e-a-armadilha-do-getuid)).

### 2. `mc run` pendurado para sempre

`recv_report` usava `read_to_string`, que espera pelo EOF — e o init mantém o pipe aberto até ao `exec`. **Cura:** uma só `read` ([capítulo 14](mc-ciclo-de-vida.html#o-protocolo-do-relatorio-e-o-bug-do-eof)). Foi apanhado por um `timeout` — sem ele, o teste teria ficado pendurado também.

### 3. `Command::output()` bloqueado 3 minutos

Um `mc create` num teste de integração deixava o supervisor a segurar o stdout do chamador. **Cura:** stdio em ficheiro no `create`. Um teste que só verificasse «devolve 0» **nunca** teria visto isto — o comando devolvia 0; era o *leitor* que ficava preso.

### 4. `mc run` devolvia 255 em vez de 7

`wait_stopped` confiava em «o PID morreu» (`reconcile`) antes de o supervisor gravar o exit code — uma corrida de milissegundos. Os testes passavam sozinhos e falhavam em paralelo. **Cura:** esperar pelo estado **persistido** `stopped`, e só desistir se o supervisor desaparecer sem gravar, após uma folga de 2 s.

### 5. Uma limpeza «inofensiva» que partiu 6 testes

Uma correcção de *lint* do clippy (`or_else(|_| Err(…))` → `?`) revelou que o código antigo **escondia** um erro: `resolve_in_root(…, create=false)` num ficheiro que ainda não existia devolvia `NotFound`, e um `.unwrap_or_else` engolia-o e caía num caminho alternativo por acaso. Com o `?` honesto, o teste ficou vermelho. **Lição:** um `unwrap_or_else` sobre um erro é um convite para o esconder — e os testes de integração são o que apanha a diferença.

### 6. O meu próprio demo estava conceptualmente errado

O primeiro `ns_demo` afirmava que o hostname do host ficava «inalterado» comparando o do *pai* antes e depois. Falhava — porque o pai **também** entrou no UTS namespace novo (o `unshare` aplica-se a quem chama). A afirmação certa só se pode fazer **de fora**: o teste de integração corre o binário e compara o `hostname` do host antes e depois. **Lição:** verifica a propriedade no sítio onde ela é observável.

!!! delonix "O padrão nos seis"
    Nenhum destes teria sido apanhado por leitura ou por um teste unitário: são **interacções** — com o kernel, entre processos, entre ordens de operações. É por isto que o delonix tem uma bateria E2E que corre a CLI real e um arnês de caos, e porque a regra da casa é «prova medida, não afirmada».

## O que **não** foi validado

Dizê-lo é parte do trabalho:

- **Uma só máquina, um só kernel** (Linux 7.0, cgroup v2 puro, `busybox` estático). Não se testou em kernels antigos, cgroup v1 ou híbrido, nem noutras distros.
- **A suite oficial do OCI** (`runtime-tools`) **não** foi corrida. A contra-prova é **um** bundle no `runc`, não conformidade total.
- **Um só uid mapeado.** Imagens com ficheiros de vários donos (e `USER` não-root) não foram testadas.
- **Sem `seccomp` nem rede** — recusa-se o primeiro; do segundo só há o `lo`.
- **CI**: os testes E2E dependem de user namespaces e `busybox`; no GitHub Actions o *runner* pode restringi-los (o teste salta e diz). **Verifica o resultado do primeiro run** em vez de assumir.
- **Sem *fuzzing*.** O `unpack` de arquivos e o parser da spec são superfícies óbvias para `cargo fuzz` — fica como exercício.

## Corre tu

```bash
git clone https://github.com/angolardevops/delonix-rust-tutorial && cd delonix-rust-tutorial
cargo test --workspace                       # o que a CI corre
scripts/demo.sh                              # regenera TODAS as saídas deste capítulo
systemd-run --user --scope -p Delegate=yes scripts/demo.sh   # inclui cgroups (OOM, pids)
```
