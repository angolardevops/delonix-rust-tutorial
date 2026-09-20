#!/usr/bin/env bash
# Corre os cenários do capítulo do mini-projecto e grava a saída REAL em content/outputs/.
# É essa saída que o tutorial mostra: se o comportamento mudar, o site muda com ele.
set -uo pipefail
cd "$(dirname "$0")/.."
cargo build -q --release -p minicontainer -p examples
mc=$PWD/target/release/mc
out=content/outputs; mkdir -p "$out"
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
export MC_ROOT=$work/state
run() { local name=$1; shift; { echo "\$ $*"; "$@" 2>&1; echo "[exit $?]"; } > "$out/$name.txt"; }

scripts/make-rootfs.sh "$work/b" >/dev/null
$mc spec "$work/b" -- /bin/sh -c 'echo "pid=$$ host=$(hostname) uid=$(id -u)"; grep CapBnd /proc/self/status; ls /dev'
run 01-run-basico $mc run demo -b "$work/b"

$mc spec "$work/b" -- /bin/sh -c 'exit 7'
{ echo "\$ mc run e -b bundle   # o processo faz 'exit 7'"; $mc run e -b "$work/b"; echo "[exit $?]"; } > $out/02-exit-code.txt 2>&1
$mc spec "$work/b" -- /bin/nope
{ echo "\$ mc run n -b bundle   # binário inexistente"; $mc run n -b "$work/b" 2>&1; echo "[exit $?]"; } > $out/03-exit-127.txt

$mc spec "$work/b" -- /bin/sh -c 'echo arrancou; sleep 30'
{
 echo "\$ mc create life -b bundle";  $mc create life -b "$work/b"; echo "[exit $?]"
 echo "\$ mc state life";             $mc state life
 echo "\$ mc start life";             $mc start life; echo "[exit $?]"
 echo "\$ mc list";                   $mc list
 echo "\$ mc logs life";              $mc logs life
 echo "\$ mc create life -b bundle   # duplicado";  $mc create life -b "$work/b" 2>&1; echo "[exit $?]"
 echo "\$ mc delete life             # ainda vivo";   $mc delete life 2>&1; echo "[exit $?]"
 echo "\$ mc kill life";              $mc kill life; echo "[exit $?]"; sleep 0.4
 echo "\$ mc state life";             $mc state life
 echo "\$ mc delete life";            $mc delete life; echo "[exit $?]"
 echo "\$ mc state life";             $mc state life 2>&1; echo "[exit $?]"
} > $out/04-lifecycle.txt 2>&1

$mc spec "$work/b" -- /bin/sh -c 'touch /x; echo "touch /x -> rc=$?"; touch /tmp/x && echo "touch /tmp/x -> ok"'
jq '.root.readonly=true' "$work/b/config.json" > "$work/c" && mv "$work/c" "$work/b/config.json"
run 05-readonly $mc run ro -b "$work/b"
jq '.linux.seccomp={defaultAction:"SCMP_ACT_ALLOW"}' "$work/b/config.json" > "$work/c" && mv "$work/c" "$work/b/config.json"
run 06-seccomp-recusado $mc run sc -b "$work/b"

jq 'del(.linux.seccomp) | .root.readonly=false | .linux.resources={memory:{limit:33554432},pids:{limit:16}} | .process.args=["/bin/sh","-c","cat /proc/self/cgroup"]' "$work/b/config.json" > "$work/c" && mv "$work/c" "$work/b/config.json"
{ echo "\$ mc run cg -b bundle   # limites pedidos, SEM scope delegado"; $mc run cg -b "$work/b" 2>&1; echo "[exit $?]"; } > $out/07-cgroup-sem-delegacao.txt
{ echo "\$ systemd-run --user --scope -p Delegate=yes mc run cg -b bundle"; systemd-run --user --scope -q -p Delegate=yes $mc run cg2 -b "$work/b" 2>&1; echo "[exit $?]"; } > $out/08-cgroup-delegado.txt
jq '.process.args=["/bin/sh","-c","a=aaaaaaaaaaaaaaaa; while :; do a=$a$a$a; done"]' "$work/b/config.json" > "$work/c" && mv "$work/c" "$work/b/config.json"
{ echo "\$ systemd-run --user --scope -p Delegate=yes mc run oom -b bundle   # memory.max=32M; o processo enche a memória"; systemd-run --user --scope -q -p Delegate=yes $mc run oom -b "$work/b" 2>&1; echo "[exit $?]"; } > $out/09-oom.txt
jq '.process.args=["/bin/sh","-c","for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do sleep 5 & done; echo forked; wait"]' "$work/b/config.json" > "$work/c" && mv "$work/c" "$work/b/config.json"
{ echo "\$ systemd-run --user --scope -p Delegate=yes mc run pids -b bundle   # pids.max=16; o processo faz fork de 20"; timeout 20 systemd-run --user --scope -q -p Delegate=yes $mc run pids -b "$work/b" 2>&1 | head -3; } > $out/10-pids.txt

scripts/make-oci-layout.sh "$work/b" "$work/img" > /dev/null
{
 echo "\$ find oci-layout -type f   # (digests abreviados)"; (cd "$work/img" && find . -type f | sort | sed 's/sha256\/\(.\{12\}\).*/sha256\/\1…/')
 echo "\$ mc unpack layout bundle2";  $mc unpack "$work/img" "$work/b2"; echo "[exit $?]"
 echo "\$ ls bundle2/rootfs/etc   # 'removido-pela-layer-2' foi apagado pelo whiteout"; ls "$work/b2/rootfs/etc"
 echo "\$ jq -c .process bundle2/config.json"; jq -c '.process' "$work/b2/config.json"
 echo "\$ mc run img -b bundle2"; $mc run img -b "$work/b2"; echo "[exit $?]"
} > $out/11-imagem-oci.txt 2>&1
{
 echo "\$ cat index.json"; jq -c . "$work/img/index.json"
 m=$(jq -r '.manifests[0].digest' "$work/img/index.json" | cut -d: -f2)
 echo "\$ cat blobs/sha256/<manifest>"; jq . "$work/img/blobs/sha256/$m"
 c=$(jq -r '.config.digest' "$work/img/blobs/sha256/$m" | cut -d: -f2)
 echo "\$ cat blobs/sha256/<config>"; jq . "$work/img/blobs/sha256/$c"
} > $out/14-oci-json.txt 2>&1
{ echo "\$ mc spec bundle -- /bin/sh   # gera um config.json mínimo"; $mc spec "$work/b" -- /bin/sh; jq . "$work/b/config.json"; } > $out/15-config-json.txt 2>&1
cp -r "$work/img" "$work/img-bad"
l1=$(jq -r '.layers[0].digest' "$work/img-bad/blobs/sha256/$(jq -r '.manifests[0].digest' "$work/img-bad/index.json" | cut -d: -f2)" | cut -d: -f2)
echo x >> "$work/img-bad/blobs/sha256/$l1"
{ echo "\$ echo x >> blobs/sha256/<layer1>   # adulterar 1 byte"; echo "\$ mc unpack layout-adulterado bundle3"; $mc unpack "$work/img-bad" "$work/b3" 2>&1 | sed 's/[0-9a-f]\{64\}/<sha256>/g'; echo "[exit ${PIPESTATUS[0]}]"; } > $out/12-digest-adulterado.txt

if command -v runc >/dev/null; then
  jq '.linux.namespaces=[{type:"pid"},{type:"mount"},{type:"uts"},{type:"ipc"},{type:"network"},{type:"user"}] | .linux.uidMappings=[{containerID:0,hostID:'$(id -u)',size:1}] | .linux.gidMappings=[{containerID:0,hostID:'$(id -g)',size:1}] | .process.user={uid:0,gid:0}' "$work/b2/config.json" > "$work/b2/config.runc.json"
  cp "$work/b2/config.runc.json" "$work/b2/config.json"
  { echo "\$ runc --version | head -1"; runc --version | head -1
    echo "\$ runc run rc   # o MESMO bundle que o mc acabou de correr (+ mapeamentos de user ns)"
    (cd "$work/b2" && timeout 30 runc --root "$work/runc" run rc 2>&1); echo "[exit $?]"; } > $out/13-runc-mesmo-bundle.txt
fi
sed -i "s#$work/b2#bundle2#g; s#$work/b3#bundle3#g; s#$work/b#bundle#g; s#$mc#mc#g; s#[0-9a-f]\{64\}#&#g; s#$work#…#g" $out/*.txt
echo "saídas gravadas em $out"; ls $out
