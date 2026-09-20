---
title: Rust para sistemas distribuídos
slug: sistemas-distribuidos
summary: Reconciliação de 3 vias, idempotência, retentativas com retoma, estado sem corridas e erros com classe — os padrões do delonix.
part: 3
order: 6
time: 40 min de leitura
---

# Rust para sistemas distribuídos

O delonix é um motor de **um nó** — mas foi desenhado com os padrões de um sistema distribuído, porque um nó **já é** um sistema distribuído: vários processos (CLI, supervisor, holder de rede, servidor CRI) a mexer no mesmo estado, a falhar a meio, a serem reiniciados por um `systemd` ou por um reboot. Tudo o que se segue escala de um nó para uma frota.

Os problemas são sempre os mesmos, e este capítulo dá uma resposta a cada:

| Problema | Padrão | Onde |
|---|---|---|
| «O que é que devo mudar?» | **Reconciliação** (estado desejado vs actual) | secção 1 |
| «E se aplicar duas vezes?» | **Idempotência** | secção 2 |
| «A rede falhou a meio» | **Retentativas com backoff + retoma** | secções 3-4 |
| «Dois processos escreveram ao mesmo tempo» | **Escrita atómica + `flock`** | secção 5 |
| «Como sei o que correu mal sem ler a mensagem?» | **Erros com classe** | secção 6 |

Todos os exemplos vivem em [`examples/src/ch06_distribuido.rs`](https://github.com/angolardevops/delonix-rust-tutorial/blob/main/examples/src/ch06_distribuido.rs), com testes.

## 1. Reconciliação: uma função pura

Um sistema **declarativo** aceita «isto é o que eu quero» e descobre sozinho o que fazer. O coração é uma função que compara o desejado com o real e devolve as diferenças. A decisão de design que mais importa: **essa função é pura**.

{{snippet:reconcile-doc}}

Pura quer dizer: recebe os dois lados *já lidos*, devolve uma lista de mudanças, **nunca** abre um ficheiro nem corre um comando. É isso que torna os casos difíceis testáveis como dados, em microssegundos, sem estado do host. Uma versão minimal, com o mesmo raciocínio:

{{file:examples/src/ch06_distribuido.rs#plan}}

### Porque 3 vias, e não 2

Comparar só *desejado* contra *actual* não distingue duas situações opostas:

- «apaguei este campo do manifesto» → **reverter**;
- «alguém pôs este campo à mão» → **não tocar**.

Uma comparação de 2 vias tem de escolher uma — e erra na outra metade dos casos: ou reverte tudo o que um humano fez com `container update`, ou nunca honra uma remoção. O terceiro lado é o **último estado que aplicámos**, guardado no próprio recurso (a mesma anotação `last-applied` do `kubectl`). O teste mostra os três casos de uma vez:

```rust
let desired = f(&[("image", "nginx:2")]);
let actual  = f(&[("image", "nginx:1"), ("memory", "64M"), ("debug", "on")]);
let last    = f(&[("image", "nginx:1"), ("memory", "64M")]);
// image  → mudou no manifesto        → Update
// memory → era nosso e saiu          → reverter (valor vazio)
// debug  → nunca foi nosso           → NÃO tocar
```

E o motor real — `plan()` do `delonix-stack`, com a mesma estrutura mais o que uma frota exige (posse por *stack*, campos «frios» que forçam recriação, adopção de recursos criados à mão):

{{snippet:reconcile-plan}}

!!! delonix "A posse é o que separa um `apply` seguro de um perigoso"
    Repara em `if owner != stack { … Conflict }`: dois *stacks* a convergir o mesmo recurso fariam-no oscilar entre duas formas a cada `apply`. E o `--prune` só apaga o que tem **a nossa etiqueta** — um recurso criado à mão é invisível, «porque um `apply` nunca deve considerar apagar o que não criou».

!!! tip "Fail-closed na recriação"
    Quando a diferença está num campo que só se muda recriando o recurso (`-/+`), o motor **recusa sem `--replace <Kind>/<nome>`**, e recusa **antes** da primeira criação: um `apply` que falhasse a meio deixaria a stack meio convergida *e* com erro.

## 2. Idempotência: a prova é «zero escritas»

Aplicar N vezes tem de dar o mesmo estado que aplicar uma. Não basta o resultado ser igual — **a segunda aplicação não deve escrever nada**:

{{file:examples/src/ch06_distribuido.rs#apply}}

O teste `apply_is_idempotent` afirma `writes == 1` depois de três `apply`. Repara na pegadinha que o motor pagou: um `apply` que «não faz nada» também deixa o PID de um container intacto — por isso o cenário de caos do delonix **não** verifica só que o PID não mudou, verifica que o *registo* mudou (`memory_max`) **e** que o `stack plan` seguinte não tem nada a propor. Um teste que passa com o código apagado não prova nada.

## 3. Retentativas: backoff exponencial, com tecto

{{file:examples/src/ch06_distribuido.rs#backoff}}

Três decisões escondidas em duas funções pequenas:

- **Tecto** (`cap`): sem ele, uma falha longa dorme horas.
- **`checked_shl` + `saturating_mul`**: `backoff(500, …)` não dá pânico nem volta a zero — o teste prova-o.
- **Só se retenta o que faz sentido**: um 404 não melhora à quinta tentativa; um 503 sim.

Em produção acrescenta-se *jitter* (aleatoriedade) para uma frota não retentar em uníssono — aqui fica de fora para o teste ser determinístico.

## 4. Retomar um download: o servidor responde a *outra pergunta*?

O bug que deu origem a isto é real e ensina muito. `delonix vm pull` de 276 MiB morria aos 8 minutos numa ligação de 416 KB/s — e a tentativa seguinte **recomeçava do byte zero**. Abaixo de ~600 KB/s a imagem *nunca* acabava. A cura é `Range: bytes=<n>-` a partir do que já está em memória. O que torna isto subtil:

{{file:examples/src/ch06_distribuido.rs#range}}

Um servidor pode responder ao pedido de retoma de **três** maneiras, e só uma é retomável: `206` no offset pedido (retoma), `206` noutro offset (**responde a outra pergunta** — colar duplicaria o prefixo e a corrupção só apareceria no digest, depois de pagar o download inteiro), ou `200` (ignorou o header: recomeça). E outra armadilha: o `Content-Length` de um `206` é o tamanho do **fragmento**, não do blob — o total vem do `/<total>` do `Content-Range`.

O que torna seguro *costurar* dois intervalos é o **digest verificado no fim**: bytes de duas respostas ou dão o hash publicado, ou o download é descartado. Comprova-se ao vivo no delonix contra um registo que corta a ligação a meio.

## 5. Estado em disco sem corridas

Vários processos escrevem no mesmo estado. Duas ferramentas, e precisas das duas.

### Escrita atómica

{{file:examples/src/ch06_distribuido.rs#atomic}}

Escreve num temporário **no mesmo directório** e faz `rename` (atómico no POSIX): um leitor vê o ficheiro antigo inteiro ou o novo inteiro, nunca metade. Repara na **ordem**: `fsync` do conteúdo *antes* do `rename`. A versão de produção do delonix vai mais longe:

{{snippet:write-atomic}}

Quatro detalhes que só se aprendem a perder dados: nome do temporário **único por escritor** (senão dois escritores intercalam bytes e o `rename` *publica a corrupção*), modo definido **na criação** (`OpenOptions::mode`) e não `chmod` depois (janela em que outro utilizador abre o ficheiro), `fsync` do **directório** depois do `rename`, e remover o temporário se falhar.

### `flock`: read-modify-write

A escrita atómica protege contra ficheiros rasgados, **não** contra o *lost update*: dois processos lêem `v=1`, ambos escrevem `v=2`, e um incremento perdeu-se. A cura é um lock **e** reler **sob** o lock:

{{snippet:store-update}}

O comentário no código é o ponto: *«Re-read UNDER the lock: between any earlier read and the flock another process may have written»*. O `minicontainer` tem o mesmo padrão em [`Store::update`](mc-ciclo-de-vida.html), com um teste que lança 16 *threads* a incrementar — sem o lock perdia escritas.

!!! danger "Um `flock` esquecido custou dados"
    Todos os caminhos de mutação da firewall do delonix contornavam o `flock`: `load` → mutar → aplicar no kernel → `save`. Dois comandos concorrentes aplicavam ambos no kernel, mas só o último `save` sobrevivia no disco — a regra «perdedora» ficava viva no `nft` e desaparecia em silêncio no próximo `container start`. Seis pontos de mutação, todos corrigidos para `Store::update`.

## 6. Erros com classe

{{file:examples/src/ch06_distribuido.rs#exit}}

O código de saída é **contrato**; a mensagem é para humanos e é traduzível. Uma tabela pequena, num só sítio — e um teste que a fixa. Para um lote (`rm a b c` onde vários falharam): uma classe única **mantém-se**; um lote **misto** cai no genérico, porque escolher a classe do primeiro faria o resultado depender da ordem em que escreveste os ids.

## E async? `tokio`, gRPC e o resto

Tudo acima é **síncrono** de propósito — o delonix é daemonless e a maioria do motor não precisa de um *runtime* assíncrono. Onde precisa (o servidor CRI, a API de gestão, o cliente de registos) usa `tokio` + `tonic` (gRPC) + `hyper`, e mantém-nos **confinados** aos crates de interface, longe da lógica pura. Regras que valem para qualquer sistema em Rust:

- **Async só onde há I/O concorrente**; lógica de domínio é síncrona e pura (testa-se sem *runtime*).
- **Nunca bloquear dentro de uma *task*** (`std::thread::sleep`, I/O síncrono pesado): usa `spawn_blocking`.
- **Timeouts em tudo o que toca na rede** — um pedido sem *deadline* é um *hang* à espera de acontecer. O delonix corrige `read_to_string` de sockets com tecto (o cliente de controlo do holder passou de 5 s para 30 s ao medir 30 *attaches* concorrentes: 15 falhavam).
- **Observabilidade por padrões abertos** (`tracing` → OpenTelemetry, métricas Prometheus), nunca `println!` numa biblioteca: quem imprime é a interface.

## Verifica o que aprendeste

```bash
cargo test -p examples ch06        # 9 testes
```

Exercício: torna o `plan` capaz de **remover** um campo que nunca esteve em `last_applied` mas que o manifesto marca explicitamente com `null`. Que teste escreves primeiro?
