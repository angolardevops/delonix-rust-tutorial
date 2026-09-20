#!/usr/bin/env bash
# Cria um bundle mínimo a partir do busybox ESTÁTICO do host: scripts/make-rootfs.sh <bundle>
set -euo pipefail
bundle=${1:?usage: make-rootfs.sh <bundle>}
bb=$(command -v busybox)
file "$bb" | grep -q 'statically linked' || { echo "busybox tem de ser estático" >&2; exit 1; }
mkdir -p "$bundle/rootfs"/{bin,proc,dev,tmp,etc}
cp "$bb" "$bundle/rootfs/bin/busybox"
for applet in sh ls cat echo hostname id sleep ps mount touch head dd yes; do
  ln -sf busybox "$bundle/rootfs/bin/$applet"
done
echo "root:x:0:0:root:/:/bin/sh" > "$bundle/rootfs/etc/passwd"
