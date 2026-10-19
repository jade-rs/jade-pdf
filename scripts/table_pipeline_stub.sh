#!/usr/bin/env sh
# Phase 1 用スタブ: 実際の PP-Structure パイプラインの代わりに固定 JSON を stdout へ出す。
# 使用例: `--ocr-table-pipeline scripts/table_pipeline_stub.sh --ocr-image table.png`
# または `--ocr-from-pdf` と併用（先頭ページ PNG が渡る）
# 引数の画像パスは無視する。
printf '%s\n' '{"html":"<table><tr><td>stub</td></tr></table>"}'
