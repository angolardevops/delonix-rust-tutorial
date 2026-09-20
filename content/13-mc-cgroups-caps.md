---
title: "4 · cgroups e capabilities"
slug: mc-cgroups-caps
summary: Limites de memória, pids e CPU com cgroups v2 rootless, detecção de OOM, e reduzir o poder do processo ao conjunto OCI.
part: 4
order: 13
time: 25 min de leitura
---

# 4 · cgroups e capabilities

Depois de isolar o que o processo *vê*, limitamos o que ele *gasta* e o que *pode fazer*.

## Traduzir a spec para ficheiros do cgroup

O `linux.resources` da spec vira escritas em ficheiros do cgroup v2. Isto é **lógica pura**, separada do I/O — testável sem tocar em `/sys`:

{{file:minicontainer/src/cgroup.rs#limits}}

Duas escolhas a notar:

- **`memory.swap.max = 0` acompanha `memory.max`.** Sem isso o kernel «cumpre» o limite empurrando páginas para *swap*, e o container nunca é morto por OOM — só fica lento.
- **`-1` significa `max`** (sem limite), tal como na spec.

E o teste, sem cgroup nenhum:

```rust
let res = Resources {
    memory: Some(Memory { limit: Some(64 << 20) }),
    pids: Some(Pids { limit: 32 }),
    cpu: Some(Cpu { quota: Some(50_000), period: Some(100_000) }),
};
let f = limit_files(&res);
assert!(f.contains(&("memory.max", "67108864".to_owned())));
assert!(f.contains(&("cpu.max", "50000 100000".to_owned())));
```

## Criar o cgroup — e recusar se não der

Aqui está a parte que dá dor de cabeça a toda a gente que faz rootless. Já vimos no [capítulo do Linux](linux-containers.html#o-problema-o-cgroup-tem-de-ser-teu): só numa subárvore **delegada** ao teu utilizador é que se criam filhos com controladores.

{{file:minicontainer/src/cgroup.rs#cg-create}}

Os passos, pela ordem que o kernel exige:

1. **`mc-mgr`** — cria um cgroup filho «gestor» e **move-se para lá** (`cgroup.procs` ← `0` = «eu próprio»). É a regra *no internal processes*: só assim o pai fica sem processos directos e pode activar controladores.
2. **Verifica os controladores disponíveis** (`cgroup.controllers`) contra os que os limites pedem. Falta um? **Erro**, com o remédio.
3. **`cgroup.subtree_control` ← `+memory +pids +cpu`** — activa-os para os filhos.
4. **`mc-<id>`** — o cgroup do container, e escreve-se cada limite.

E o comportamento quando o host **não** deixa — sem delegação:

{{out:07-cgroup-sem-delegacao}}

O `EBUSY` (`Device or resource busy`) é o kernel a dizer «este cgroup ainda tem processos». A mensagem não se fica pelo erro cru: diz o **remédio**. Comparação com uma sessão delegada:

{{out:08-cgroup-delegado}}

O container está agora em `.../mc-cg2` — o cgroup que criámos.

!!! delonix "Fail-closed, outra vez"
    A alternativa fácil era «se não conseguir, segue sem limites». **Não**: o operador pediu `memory: 32M`, o container corre sem tecto, e ninguém sabe. O delonix mediu isto numa VM limpa: `-m 128M --cpus 0.5` inertes numa sessão SSH normal. Recusar com o remédio é a única resposta honesta — e é a regra de ouro do capítulo 10 aplicada a recursos.

## Os limites a funcionar

Numa sessão com cgroup delegado, um processo que enche a memória contra `memory.max = 32 MiB`:

{{out:09-oom}}

Saída `137` = `128 + 9` (SIGKILL) — a convenção de shell para «morto por sinal 9». E o `mc` **diz porquê**: «OOM-killed». Um `137` sozinho lê-se igual para um OOM e para um `kill -9`, e os remédios são opostos. E o limite de processos:

{{out:10-pids}}

`can't fork: Resource temporarily unavailable` — o `EAGAIN` que o kernel devolve quando `pids.max` está atingido.

### Detectar o OOM: o cgroup desaparece com o container

Aqui está uma armadilha que o delonix mediu com atenção, e o `minicontainer` respeita. **O cgroup de um container desaparece no instante em que ele morre.** Depois disso, `memory.events` (onde o kernel conta `oom_kill`) já não é legível. Portanto **não há detecção post-mortem possível**: tem de ser lida **ao vivo**, por quem vive tanto quanto o container — o supervisor, antes de remover o cgroup:

```rust
// no supervisor, depois do waitpid e ANTES de remover o cgroup:
let oom = cg.as_ref().is_some_and(|c| c.oom_kills() > 0);
store.update(id, |st| { st.status = Status::Stopped; st.exit_code = Some(code); st.oom_killed = oom; Ok(()) })?;
```

E a leitura em si:

```rust
pub fn oom_kills(&self) -> u64 {
    fs::read_to_string(self.path.join("memory.events"))
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("oom_kill ")?.trim().parse().ok()))
        .unwrap_or(0)
}
```

!!! warning "Limitação conhecida"
    `oom_kills() > 0` é suficiente aqui porque o cgroup é **novo** e só tem este container. O delonix compara uma **subida** do contador contra uma linha de base lida antes do `waitpid` — um cgroup reutilizado traz a contagem antiga, e `> 0` chamaria OOM a um `kill -9`. Lê também `memory.events.local` (não o hierárquico), porque o cgroup de um nó Kubernetes-em-container contém uma árvore systemd inteira.

## Capabilities: reduzir o tecto

Dentro do user namespace, o init tem **todas** as capabilities. Para a carga do utilizador queremos as 14 da OCI e mais nenhuma. Retira-se do *bounding set* (o tecto herdado por qualquer processo filho):

{{file:minicontainer/src/container.rs#caps}}

Porque o **bounding set** e não as capabilities efectivas? Quando um processo de uid 0 faz `exec`, as suas capabilities permitidas ficam `= bounding set`. Ao reduzir o tecto **antes** do `exec`, o processo do utilizador nasce já limitado — e nunca as pode reconquistar. O teste verifica o valor exacto:

```rust
assert_eq!(bnd, "00000000a80425fb", "bounding set != OCI default");
```

`a80425fb` são as 14 do [`capsh --decode`](linux-containers.html): `chown, dac_override, fowner, fsetid, kill, setgid, setuid, setpcap, net_bind_service, net_raw, sys_chroot, mknod, audit_write, setfcap`.

Duas notas de precisão:

- `EINVAL` é **tolerado** em `PR_CAPBSET_DROP`: uma capability que o kernel não conhece (a lista `0..=cap_last_cap` pode incluir números que esta versão não tem) não é erro.
- A seguir vem `PR_SET_NO_NEW_PRIVS`: o processo (e os seus filhos) **nunca** ganha privilégios num `exec` — nem por *setuid*, nem por capabilities de ficheiro.

## O que falta (e o delonix faz)

| | `minicontainer` | delonix |
|---|---|---|
| `cpuset`, `io.weight`, `cpu.weight` | ❌ | ✅ (best-effort: só se delegados) |
| `hugepages`, `unified` | ❌ | ✅ |
| Hierarquia do kubelet (scope transitório pelo systemd) | ❌ | ✅ (`StartTransientUnit`) |
| `oom_score_adj` | ❌ | ✅ |
| `ambient`/`inheritable` caps, CDI de GPU | ❌ | ✅ |
| **seccomp** | ❌ recusa | ✅ allowlist + `clone3→ENOSYS` |

## Verifica

```bash
cargo test -p minicontainer cgroup::                     # tradução de limites
cargo test -p minicontainer --test e2e drops_capabilities
systemd-run --user --scope -p Delegate=yes target/release/mc run oom -b /tmp/b   # com limites no config.json
```

Falta a peça que junta tudo: [o ciclo de vida e o estado](mc-ciclo-de-vida.html).
