#!/bin/sh
# Linuxの専用委譲先に移動し、通常ユーザーのまま実カーネル回帰を実行する。
set -eu
test "$(uname -s)" = Linux
parent="/sys/fs/cgroup/jade-pdf2docx-test-$$"
uid=$(id -u)
gid=$(id -g)
cargo=$(command -v cargo)
sudo mkdir "$parent"
cleanup() {
    sudo rmdir "$parent/runner" "$parent"
}
trap cleanup EXIT
printf '+memory\n' | sudo tee "$parent/cgroup.subtree_control" >/dev/null
sudo mkdir "$parent/runner"
sudo chown "$uid:$gid" "$parent" "$parent/cgroup.procs" "$parent/cgroup.subtree_control"
# 所属変更の共通祖先への権限も必要なので、rootでrunner leafへ移動してから降格する。
# 全テストとcargoは元のuidで動かす。親CLIは変換群の外側で生存する。
sudo --preserve-env=PDFIUM_LIB_PATH sh -c '
    set -eu
    parent=$1; uid=$2; gid=$3; cargo=$4
    task_home=$5; task_path=$6; cargo_home=$7; rustup_home=$8; target=$9
    printf "%s\n" "$$" > "$parent/runner/cgroup.procs"
    exec setpriv --reuid="$uid" --regid="$gid" --init-groups env \
        HOME="$task_home" PATH="$task_path" CARGO_HOME="$cargo_home" \
        RUSTUP_HOME="$rustup_home" CARGO_TARGET_DIR="$target" \
        CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
        JADE_PDF2DOCX_TEST_CGROUP_PARENT="$parent" sh -c '\''
            set -eu
            "$1" test --locked --bin jade-pdf2docx memory_ -- --include-ignored --test-threads=1
            "$1" test --locked --test production_memory -- --include-ignored --test-threads=1
        '\'' sh "$cargo"
' sh "$parent" "$uid" "$gid" "$cargo" "$HOME" "$PATH" \
    "${CARGO_HOME:-$HOME/.cargo}" "${RUSTUP_HOME:-$HOME/.rustup}" "${CARGO_TARGET_DIR:-target}"
