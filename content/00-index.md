---
title: Início
slug: index
summary: Aprende Rust de sistemas e contribui para o delonix-runtime — do básico ao teu próprio runtime OCI, com código testado.
part: 0
order: 0
---

<section class="hero">
<span class="eyebrow">Tutorial aberto · pt-AO · Apache-2.0</span>


<h1>Programa <em>containers</em> em Rust — e contribui para o <em>delonix-runtime</em></h1>

<p>Um caminho completo para quem quer escrever software de sistemas em Rust: dos conceitos básicos ao código real de um motor de containers e microVMs, e no fim um <b>runtime OCI funcional</b> — o teu — construído passo a passo, testado e validado contra o <code>runc</code>.</p>

<div class="cta">
<a class="btn pri" href="rust-essencial.html">Começar pelo Rust →</a>
<a class="btn" href="mc-visao.html">Ir directo ao mini-projecto</a>
<a class="btn" id="hero-search" href="#" onclick="document.getElementById('search-open').click();return false;">Pesquisar <kbd>Ctrl K</kbd></a>
</div>
</section>

<div class="stats">
<div class="stat"><b>22</b><span>crates reais do motor estudados</span></div>
<div class="stat"><b>22</b><span>testes do minicontainer (15 unit + 7 E2E)</span></div>
<div class="stat"><b>0</b><span>daemons, 0 root — tudo rootless</span></div>
<div class="stat"><b>OCI + CRI</b><span>a norma que o motor cumpre</span></div>
</div>

## Como este tutorial funciona

Cada afirmação tem prova. Há **três tipos de conteúdo verificável**, e o site marca-os de forma diferente para saberes sempre de onde vem o que lês:

| Marca | O que é | Porque podes confiar |
|---|---|---|
| Bloco com título `delonix-runtime · caminho · linhas` | Excerto do motor real, de um **commit fixo** | Tem *permalink* para o GitHub — não muda debaixo de ti |
| Bloco com título `minicontainer/…` ou `examples/…` | Código deste repositório | Compila e é testado em CI; se estiver errado o pipeline fica vermelho |
| Terminal com «saída real · medida neste host» | Saída de comandos corridos a sério | Gerada por `scripts/demo.sh`; inclui os erros e os códigos de saída |

!!! delonix "A regra da casa"
    O delonix persegue uma regra que este tutorial também segue: **prova medida, não afirmada**. Um relatório com só a parte boa é um relato desonesto. Por isso, onde algo **não** foi validado — ou onde o `minicontainer` faz menos do que o delonix — está dito, com a razão.

## O caminho

<div class="cards">
<a class="card" href="rust-essencial.html"><small>1 · Fundamentos</small><b>Rust essencial</b><p>Ownership, enums, Result, traits — com exemplos de motor.</p></a>
<a class="card" href="rust-idiomatico.html"><small>1 · Fundamentos</small><b>Rust idiomático</b><p>Newtypes, typestate, builders: estados inválidos que não compilam.</p></a>
<a class="card" href="linux-containers.html"><small>2 · Linux</small><b>O que é um container</b><p>Namespaces, cgroups v2, capabilities, rootless — com saídas reais.</p></a>
<a class="card" href="oci.html"><small>2 · Linux</small><b>OCI: imagem e runtime</b><p>Layers, digests, whiteouts, bundle e ciclo de vida.</p></a>
<a class="card" href="cri.html"><small>2 · Linux</small><b>CRI e o Kubernetes</b><p>Como o kubelet fala com o runtime, e onde entra o OCI.</p></a>
<a class="card" href="sistemas-distribuidos.html"><small>3 · Sistemas</small><b>Rust para sistemas distribuídos</b><p>Reconciliação, idempotência, retentativas, estado sem corridas.</p></a>
<a class="card" href="anatomia-delonix.html"><small>3 · Sistemas</small><b>Anatomia do delonix</b><p>As 5 camadas, o gate de arquitectura, daemonless.</p></a>
<a class="card" href="projecto-completo.html"><small>3 · Sistemas</small><b>Um projecto Rust completo</b><p>Workspace, lints, CI, testes, releases, ADRs.</p></a>
<a class="card" href="mc-visao.html"><small>4 · Projecto</small><b>minicontainer</b><p>Um runtime OCI rootless em menos de 1 800 linhas (testes incluídos).</p></a>
<a class="card" href="contribuir.html"><small>5 · Contribuir</small><b>Enviar o teu primeiro PR</b><p>Worktree, gates, ADR, revisão.</p></a>
</div>

## Pré-requisitos

- **Linux** com kernel ≥ 5.10 e *user namespaces* sem privilégio (`sysctl kernel.unprivileged_userns_clone` = 1, ou o equivalente da tua distro). macOS/Windows: usa uma VM ou WSL2.
- **Rust** ≥ 1.90 (`rustup`), `git`, e para reproduzir as demos: `busybox` estático, `jq`, e opcionalmente `runc` para a contra-prova.
- Não precisas de saber Rust nem de containers — mas ajuda saber programar noutra linguagem e usar um terminal.

```bash
git clone https://github.com/angolardevops/delonix-rust-tutorial
cd delonix-rust-tutorial
cargo test --workspace          # 15 + 7 + 16 + ... testes
scripts/demo.sh                 # regenera as saídas reais citadas neste site
```

!!! tip "Pesquisa em todo o lado"
    Carrega em <kbd>Ctrl</kbd> + <kbd>K</kbd> (ou <kbd>/</kbd>) em qualquer página para pesquisar capítulos, código e saídas. Experimenta `pivot_root`, `digest` ou `cgroup`.
