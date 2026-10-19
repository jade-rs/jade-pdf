#!/bin/sh
# CI とローカルで同じ feature・lock・失敗終了条件を使用する。
set -eu

case "${1:-}" in
  default) set -- ;;
  render) set -- --features render ;;
  ocr) set -- --features ocr ;;
  ocr-onnx) set -- --features ocr-onnx ;;
  *) printf '%s\n' 'usage: check_rust_features.sh {default|render|ocr|ocr-onnx}' >&2; exit 2 ;;
esac

# 多数の統合テストbinaryとnested Cargoでもrunnerのdiskを使い切らない。
# 検査対象とdebug assertionは保持し、debug情報と増分成果物だけを省く。
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0

cargo clippy --workspace --exclude jade-pdf-diff --all-targets --locked "$@"
cargo test --workspace --exclude jade-pdf-diff --locked "$@"
