#!/usr/bin/env bash
# Saídas reais do capítulo «O que é um container» (só ferramentas do sistema, sem o mc).
set -uo pipefail
cd "$(dirname "$0")/.."; out=content/outputs; mkdir -p $out
cap() { local name=$1; shift; { for c in "$@"; do echo "\$ $c"; bash -c "$c" 2>&1; done; } > "$out/$name.txt"; }

cap 20-namespaces-do-processo 'ls -l /proc/self/ns | tail -n +2 | awk "{print \$9, \$10, \$11}"'
cap 21-unshare 'unshare --user --map-root-user --pid --fork --mount-proc --uts sh -c "hostname dentro; echo hostname=\$(hostname) pid=\$\$; id -u; ps -o pid,comm"' 'hostname   # o do host, intacto'
cap 22-uid-map-overflow 'unshare --user sh -c "cat /proc/self/uid_map; echo ---; id"' 'unshare --user --map-root-user sh -c "cat /proc/self/uid_map; id -u"'
cap 23-cgroup-v2 'stat -fc %T /sys/fs/cgroup' 'cat /sys/fs/cgroup/cgroup.controllers' 'cut -d: -f3 /proc/self/cgroup | sed "s#/app.slice/.*#/app.slice/…#"'
cap 24-cgroup-delegado 'systemd-run --user --scope -q -p Delegate=yes sh -c "echo controllers-disponiveis: \$(cat /sys/fs/cgroup\$(cut -d: -f3 /proc/self/cgroup)/cgroup.controllers); echo subtree_control=\"\$(cat /sys/fs/cgroup\$(cut -d: -f3 /proc/self/cgroup)/cgroup.subtree_control)\""'
cap 25-capabilities 'grep -E "^Cap(Inh|Prm|Eff|Bnd)" /proc/self/status' 'unshare -Ur sh -c "grep -E \"^Cap(Prm|Eff|Bnd)\" /proc/self/status"'
if command -v capsh >/dev/null; then cap 26-capsh-decode 'capsh --decode=a80425fb' 'capsh --decode=000001ffffffffff | head -c 300'; fi
sed -i 's#/home/[a-z]*/#~/#g' $out/2[0-6]-*.txt
ls $out | grep '^2'
