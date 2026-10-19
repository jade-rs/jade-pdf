#!/bin/sh
# service内でkernelの実設定を照合してから、同じPIDのままCLIへ置き換える。
set -eu
unavailable() {
    echo 'memory service limit unavailable: require cgroup v2 memory.max, swap=0 and group OOM' >&2
    exit 1
}
pool_unavailable() {
    echo 'aggregate memory pool unavailable: require matching parent slice memory.max, swap=0 and oom.group=0' >&2
    exit 1
}
pool=
if [ "${1:-}" = --pool ]; then
    [ "$#" -ge 3 ] || pool_unavailable
    pool=$2
    case "$pool" in jade-pdf2docx*.slice) ;; *) pool_unavailable ;; esac
    pool_suffix=${pool#jade-pdf2docx}
    case "${pool_suffix%.slice}" in *[!a-zA-Z0-9]*) pool_unavailable ;; esac
    [ "${#pool}" -le 128 ] || pool_unavailable
    pool_mib=$3
    case "$pool_mib" in ''|*[!0-9]*) pool_unavailable ;; esac
    [ "${#pool_mib}" -le 13 ] || pool_unavailable
    while [ "${pool_mib#0}" != "$pool_mib" ]; do pool_mib=${pool_mib#0}; done
    [ -n "$pool_mib" ] && [ "$pool_mib" -le 8796093022207 ] || pool_unavailable
    pool_bytes=$((pool_mib * 1048576))
    shift 3
fi
[ "$#" -ge 2 ] || unavailable
mib=$1
shift
case "$mib" in ''|*[!0-9]*) unavailable ;; esac
[ "${#mib}" -le 13 ] || unavailable
[ "$mib" -gt 0 ] && [ "$mib" -le 8796093022207 ] || unavailable
bytes=$((mib * 1048576))
group=
[ -r /proc/self/cgroup ] || unavailable
while IFS=: read -r hierarchy controllers path; do
    if [ "$hierarchy" = 0 ] && [ -z "$controllers" ]; then group=$path; fi
done < /proc/self/cgroup
case "$group" in /*) ;; *) unavailable ;; esac
controls=/sys/fs/cgroup$group
[ "$(cat "$controls/memory.max" 2>/dev/null)" = "$bytes" ] || unavailable
[ "$(cat "$controls/memory.swap.max" 2>/dev/null)" = 0 ] || unavailable
[ "$(cat "$controls/memory.oom.group" 2>/dev/null)" = 1 ] || unavailable
if [ -n "$pool" ]; then
    parent=${controls%/*}
    [ "${parent##*/}" = "$pool" ] || pool_unavailable
    [ "$(cat "$parent/memory.max" 2>/dev/null)" = "$pool_bytes" ] || pool_unavailable
    [ "$(cat "$parent/memory.swap.max" 2>/dev/null)" = 0 ] || pool_unavailable
    # pool全体を一括OOM対象にせず、個々のserviceを停止単位にする。
    [ "$(cat "$parent/memory.oom.group" 2>/dev/null)" = 0 ] || pool_unavailable
fi
exec "$@"
