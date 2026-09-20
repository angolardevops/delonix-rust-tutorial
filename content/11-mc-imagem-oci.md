---
title: "2 · Imagem OCI"
slug: mc-imagem-oci
summary: Desempacotar um image layout num bundle — verificar digests, aplicar layers em ordem, tratar whiteouts e gerar o config.json.
part: 4
order: 11
time: 20 min de leitura
---

# 2 · Imagem OCI

`mc unpack <layout> <bundle>` transforma uma imagem OCI numa pasta que o runtime sabe correr. Já viste o formato no [capítulo OCI](oci.html); aqui está o código.

## Ter uma imagem para desempacotar

Sem acesso a um registo (o *pull* é matéria do delonix, não do mini-projecto), `scripts/make-oci-layout.sh` **gera** um *image layout* válido a partir de um rootfs: duas layers — a segunda apaga um ficheiro da primeira com um *whiteout* e acrescenta outro — com config, manifest e index, tudo com os digests certos.

```bash
scripts/make-rootfs.sh /tmp/b
scripts/make-oci-layout.sh /tmp/b /tmp/img
mc unpack /tmp/img /tmp/b2 && mc run img -b /tmp/b2
```

{{out:11-imagem-oci}}

O que esta saída prova, ponto por ponto:

- **`Entrypoint` + `Cmd`** da imagem tornaram-se os `process.args` do bundle;
- **`Env` e `WorkingDir`** foram aplicados (`MSG=ola-da-imagem`, `cwd=/tmp`);
- **whiteout**: `removido-pela-layer-2` estava na layer 1 e **não** está no rootfs;
- e o `motd` que só existe na layer 2 lê-se `vem da layer 2`.

## O algoritmo, em dez linhas

{{file:minicontainer/src/image.rs#unpack}}

A ordem é deliberada:

1. **Verifica tudo primeiro.** O `manifest`, a `config` e **cada layer** são verificados contra os seus digests *antes* de se escrever um byte no bundle. Um erro a meio da extracção deixaria um rootfs meio-feito (e potencialmente adulterado) para trás.
2. **Aplica as layers pela ordem do manifest** — a ordem é o significado.
3. **Só no fim gera o `config.json`**, e com escrita **atómica** (`write_atomic`): um bundle ou tem config completa ou não a tem.

Repara no `.collect::<Result<_>>()?` — um iterador de `Result` recolhido para um `Result<Vec>` para à **primeira** falha. É o idioma para «faz isto a todos e aborta se algum falhar».

## Aplicar uma layer

{{file:minicontainer/src/image.rs#layer}}

Cada linha faz uma escolha de segurança:

- **`set_preserve_ownerships(false)`** — sem root não há `chown`; e mesmo com root, não devíamos deixar uma imagem escolher os donos dos ficheiros do host.
- **Whiteouts tratados antes de extrair**, e o directório-pai resolvido com [`resolve_in_root`](mc-namespaces-rootfs.html#resolver-caminhos-sem-sair-do-rootfs) (recusa `..` e symlinks): um whiteout hostil não pode apagar fora do rootfs.
- **Nodes de dispositivo são saltados** (com aviso). Uma imagem legítima não precisa de `mknod` — e uma que traga `/dev/sda` está a pedir problemas.
- **`unpack_in` do crate `tar`** recusa entradas com `..` e symlinks que escapem do destino; se devolver `false`, o `mc` falha com `UnsafePath`.
- **`make_dirs_writable`** — uma layer pode trazer directórios `0555`, e a seguinte precisa de lá escrever (somos o dono, mas sem o bit de escrita nem o dono escreve).

!!! danger "Tar-slip é a vulnerabilidade clássica de extractores"
    Um `tar` com uma entrada `../../home/user/.ssh/authorized_keys` escreve fora do destino se o extractor fizer `destino.join(entrada)`. O delonix já teve um *path traversal* em whiteouts OCI (`safe_rel` + confinamento canonicalizado), e outro no `COPY` do `build` (`safe_join`). Nunca junto caminhos vindos de um arquivo sem os validar.

## A config da imagem vira o `process`

O `config.json` gerado usa os *defaults* de um bundle nosso (`Spec::new_default`) e sobrepõe o que a imagem declara:

```json
{"terminal":false,"args":["/bin/sh","-c","cat /etc/motd; ls /etc; echo cwd=$(pwd) MSG=$MSG"],
 "env":["PATH=/bin","MSG=ola-da-imagem"],"cwd":"/tmp"}
```

Uma imagem **sem** `Entrypoint` nem `Cmd` é recusada com uma mensagem (`image has neither Entrypoint nor Cmd`) — não há o que correr.

## O que este `unpack` *não* faz

!!! warning "Limitações honestas"
    - **Só *image layout* local.** Não fala com registos (autenticação, `Accept` de media types, *fallback* de manifest lists multi-arquitectura, retoma de downloads — o [capítulo 6](sistemas-distribuidos.html) mostra a retoma).
    - **Só `sha256`** e só layers `tar` / `tar+gzip` (não `zstd`).
    - **Não escolhe a plataforma** de um *image index* multi-arch: usa o primeiro manifest de imagem.
    - **Uma cópia do rootfs por bundle.** O delonix partilha as layers entre containers com `overlayfs` montado *dentro* do user namespace — medido: 6 containers da mesma imagem em 1 MiB cada contra 17 MiB de layers.

## Verifica

```bash
cargo test -p minicontainer image::                 # digest malformado, blob adulterado
scripts/demo.sh && cat content/outputs/12-digest-adulterado.txt
```

{{out:12-digest-adulterado}}

Agora que há um rootfs no disco, falta o difícil: [isolar e arrancar](mc-namespaces-rootfs.html).
