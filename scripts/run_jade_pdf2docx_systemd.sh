#!/bin/sh
# 親CLIから全子孫までを、起動前に制限した一つのsystemd serviceへ入れる。
set -eu
usage() {
    echo 'usage: run_jade_pdf2docx_systemd.sh [--system] [--pool <jade-pdf2docxNAME.slice> <total-MiB>] [--job-token <32-hex>] [--history-dir <absolute-directory>] <MiB> <jade-pdf2docx-job-id> <absolute-jade-pdf2docx> <input> <output> [jade-pdf2docx-options...]' >&2
    exit 2
}
manager=--user
if [ "${1:-}" = --system ]; then
    manager=--system
    shift
fi
normalize_mib() {
    value=$1
    case "$value" in ''|*[!0-9]*) usage ;; esac
    # signed 64-bit byte数の範囲で扱い、shellの整数overflowを避ける。
    [ "${#value}" -le 13 ] || usage
    while [ "${value#0}" != "$value" ]; do value=${value#0}; done
    [ -n "$value" ] && [ "$value" -le 8796093022207 ] || usage
    echo "$value"
}
pool=
if [ "${1:-}" = --pool ]; then
    [ "$#" -ge 3 ] || usage
    pool=$2
    case "$pool" in jade-pdf2docx*.slice) ;; *) usage ;; esac
    pool_suffix=${pool#jade-pdf2docx}
    case "${pool_suffix%.slice}" in *[!a-zA-Z0-9]*) usage ;; esac
    [ "${#pool}" -le 128 ] || usage
    pool_mib=$(normalize_mib "$3")
    shift 3
fi
token=
if [ "${1:-}" = --job-token ]; then
    [ "$#" -ge 2 ] || usage
    token=$2
    case "$token" in *[!a-f0-9]*) usage ;; esac
    [ "${#token}" -eq 32 ] || usage
    shift 2
fi
history_dir=
if [ "${1:-}" = --history-dir ]; then
    [ "$#" -ge 2 ] && [ -n "$pool" ] && [ -n "$token" ] || usage
    history_dir=$2
    case "$history_dir" in /*) ;; *) usage ;; esac
    shift 2
fi
[ "$#" -ge 5 ] || usage
mib=$(normalize_mib "$1")
unit=$2
shift 2
case "$unit" in jade-pdf2docx-?*) ;; *) usage ;; esac
case "$unit" in *[!a-zA-Z0-9-]*) usage ;; esac
[ "${#unit}" -le 128 ] || usage
case "$1" in /*) ;; *) usage ;; esac
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
if [ -n "$pool" ]; then
    set -- --pool "$pool" "$pool_mib" "$mib" "$@"
else
    set -- "$mib" "$@"
fi
set -- /bin/sh "$script_dir/exec_in_memory_service.sh" "$@"

# system managerへsudoした場合も、変換本体は呼出元の一般ユーザーで動かす。
if [ "$manager" = --system ]; then
    uid=${SUDO_UID:-$(id -u)}
    gid=${SUDO_GID:-$(id -g)}
    case "$uid:$gid" in *[!0-9:]*|:*|*:) usage ;; esac
    set -- "--uid=$uid" "--gid=$gid" -- "$@"
else
    set -- -- "$@"
fi
if [ -n "${PDFIUM_LIB_PATH:-}" ]; then
    set -- "--setenv=PDFIUM_LIB_PATH=$PDFIUM_LIB_PATH" "$@"
fi
if [ -n "$pool" ]; then
    # 既存sliceの容量は変更しない。誤設定・未設定はservice内で拒否する。
    set -- "--slice=$pool" "$@"
fi
if [ -n "$token" ]; then
    set -- "--description=jade-pdf2docx-job-$token" "$@"
fi
if [ -n "$history_dir" ]; then
    # 受付と別のmanager子プロセスが記録する。引数を引用して環境変数展開を無効化する。
    history_property=$(python3 "$script_dir/jade_pdf2docx_job_history.py" stop-property "$history_dir" "$token" "$unit.service")
    set -- "$history_property" "$@"
fi
# systemdが設定を受理できなければそのまま失敗し、直接起動へfallbackしない。
# OOMPolicy=killはkernelのmemory.oom.group=1も設定する。
exec systemd-run "$manager" --wait --pipe --collect --service-type=exec \
    --expand-environment=no "--unit=$unit" "--working-directory=$(pwd -P)" \
    --property=MemoryAccounting=yes "--property=MemoryMax=${mib}M" \
    --property=MemorySwapMax=0 --property=OOMPolicy=kill \
    --property=KillMode=control-group --property=TimeoutStopSec=5s "$@"
