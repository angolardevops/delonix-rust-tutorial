# delonix-rust tutorial

Tutorial aberto (pt-AO) para quem quer **programar em Rust para sistemas** e **contribuir para o
[delonix-runtime](https://github.com/angolardevops/delonix-runtime)** — do conceito ao código real,
terminando num **runtime OCI rootless e sem daemon** (`minicontainer`) construído passo a passo.

**Site:** <https://angolardevops.github.io/delonix-rust-tutorial/> · pesquisa em todo o lado com `Ctrl`+`K`

## O que há aqui

| Pasta | Conteúdo |
|---|---|
| [`content/`](content) | Os capítulos em Markdown (fonte do site) |
| [`minicontainer/`](minicontainer) | O projecto final: runtime OCI em Rust (biblioteca + binário `mc`), 15 testes unitários + 7 de integração |
| [`examples/`](examples) | Todos os exemplos dos capítulos — compilados e testados (`cargo test -p examples`) |
| [`scripts/`](scripts) | `demo.sh` (regenera as saídas reais citadas), `make-rootfs.sh`, `make-oci-layout.sh`, `extract-snippets.py` |
| [`build.py`](build.py), [`site-src/`](site-src) | Gerador do site estático (Markdown → HTML, índice de pesquisa) |

## Origem do conteúdo (e porque podes confiar nele)

- **Excertos do delonix-runtime** vêm de um **commit fixo** (`content/snippets.json`, gerado por
  `scripts/extract-snippets.py`) e cada um tem *permalink* para o GitHub.
- **Código do tutorial** (`minicontainer/`, `examples/`) compila e é testado em CI.
- **Saídas de terminal** são **reais**, geradas por `scripts/demo.sh` / `scripts/demo-linux.sh`.
- O `minicontainer` corre o mesmo bundle no `runc` como contra-prova. Os limites da validação (uma máquina, uma norma parcial) estão no capítulo *Testes e validação*.

## Reproduzir

Requisitos: Linux com user namespaces sem privilégio, Rust ≥ 1.90, `busybox` estático, `jq`; opcionais: `runc` (contra-prova) e `systemd-run` (cgroups delegados).

```bash
cargo test --workspace                     # tudo o que a CI corre
scripts/demo.sh                            # regenera content/outputs/*.txt
python3 -m pip install markdown
python3 build.py --check && python3 -m http.server -d site 8000
```

## Contribuir

Encontraste um erro? Cada página do site tem «Sugerir uma correcção». Correcções de texto, testes e
exercícios resolvidos são bem-vindos — abre um PR.

## Licença

Apache-2.0 (ver [LICENSE](LICENSE)). Os excertos do delonix-runtime mantêm a licença desse projecto (Apache-2.0).
