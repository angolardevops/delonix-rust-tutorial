#!/usr/bin/env bash
# Gera um OCI *image layout* de demonstração com DUAS layers (a 2.ª apaga um ficheiro da 1.ª
# com um whiteout). Uso: make-oci-layout.sh <bundle-com-rootfs> <layout-out>
set -euo pipefail
src=${1:?bundle}/rootfs; out=${2:?layout}
mkdir -p "$out/blobs/sha256"
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
blob() { # blob <ficheiro> -> imprime digest e move para blobs/
  local h; h=$(sha256sum "$1" | cut -d' ' -f1); mv "$1" "$out/blobs/sha256/$h"; echo "sha256:$h"; }
size() { stat -c %s "$out/blobs/sha256/${1#sha256:}"; }

# layer 1: o rootfs + um ficheiro que a layer 2 vai apagar
echo "vem da layer 1" > "$src/etc/removido-pela-layer-2"
tar -C "$src" -czf "$work/l1.tgz" .
rm "$src/etc/removido-pela-layer-2"
# layer 2: whiteout + ficheiro novo
mkdir -p "$work/l2/etc"; : > "$work/l2/etc/.wh.removido-pela-layer-2"; echo "vem da layer 2" > "$work/l2/etc/motd"
tar -C "$work/l2" -czf "$work/l2.tgz" .

diff1=sha256:$(gzip -dc "$work/l1.tgz" | sha256sum | cut -d' ' -f1)
diff2=sha256:$(gzip -dc "$work/l2.tgz" | sha256sum | cut -d' ' -f1)
d1=$(blob "$work/l1.tgz"); d2=$(blob "$work/l2.tgz")

jq -n --arg a "$diff1" --arg b "$diff2" '{architecture:"amd64",os:"linux",
  config:{Entrypoint:["/bin/sh","-c"],Cmd:["cat /etc/motd; ls /etc; echo cwd=$(pwd) MSG=$MSG"],Env:["PATH=/bin","MSG=ola-da-imagem"],WorkingDir:"/tmp"},
  rootfs:{type:"layers",diff_ids:[$a,$b]}}' > "$work/config.json"
dc=$(blob "$work/config.json")
jq -n --arg dc "$dc" --argjson sc "$(size "$dc")" --arg d1 "$d1" --argjson s1 "$(size "$d1")" --arg d2 "$d2" --argjson s2 "$(size "$d2")" \
 '{schemaVersion:2,mediaType:"application/vnd.oci.image.manifest.v1+json",
   config:{mediaType:"application/vnd.oci.image.config.v1+json",digest:$dc,size:$sc},
   layers:[{mediaType:"application/vnd.oci.image.layer.v1.tar+gzip",digest:$d1,size:$s1},
           {mediaType:"application/vnd.oci.image.layer.v1.tar+gzip",digest:$d2,size:$s2}]}' > "$work/manifest.json"
dm=$(blob "$work/manifest.json")
jq -n --arg dm "$dm" --argjson sm "$(size "$dm")" '{schemaVersion:2,manifests:[{mediaType:"application/vnd.oci.image.manifest.v1+json",digest:$dm,size:$sm}]}' > "$out/index.json"
echo '{"imageLayoutVersion":"1.0.0"}' > "$out/oci-layout"
echo "layout em $out"
