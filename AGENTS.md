# AGENTS

## リポジトリの目標

このリポジトリはPDFの差分比較と **PDFからDOCXへの変換** を提供する。

## crate 構成

- `crates/jade-pdf-diff`: PDF差分比較

- `crates/jade-pdf2docx-parser`: PDF 解析（構文解析、抽出、内部中間表現への変換）
- `crates/jade-pdf2docx-docx`: 中間表現から DOCX への変換
- `crates/jade-pdf2docx-ocr`: OCR 連携レイヤ

## OCR の参照元

OCR の実装は公開済みの `https://github.com/bokuweb/ndlocr-lite-rs` を正とする。
`jade-pdf2docx-ocr` では `ndlocr-lite-rs` crate を optional feature として利用する。

## 実装ポリシー

- 保存済み入力は出典・権利・機密情報を確認した `fixtures/` の公開PDF 3件に限定し、その他はメモリ内または一時ディレクトリに生成する。
- 実装は **pure Rust** を優先し、依存を最小化した設計で進める。

## 開発ルール

- 実装は **incremental** に進め、常に小さい単位で動く状態を維持する。
- テストは **t_wada 方式の TDD**（Red -> Green -> Refactor）で進める。
- 着手時は必ず失敗するテストまたは fixture expectation を先に追加し、最小実装で Green にしてから整理する。
- Property Based Testing が有効な箇所（入力検証・変換不変条件・境界条件）は積極的に導入する。
- ファイルは責務ごとに適切に分割し、単一ファイルへの過度な集約を避ける。
- inline test（`#[cfg(test)]`）はモジュール責務に応じて多様化し、ユニット/プロパティ/回帰の観点を混在させる。
- 関数・`struct`・`field` には Rustdoc 形式（`///`）で日本語コメントを必ず付与する。
- CLIと実行例でPDFからDOCXへの変換を実行できる状態を維持する。
- 公開API・CLI・設定を変更した場合は、READMEと関連ガイドも更新する。

## テストデータ方針

保存済みfixtureには、出典と利用条件を確認した公開PDF 3件を使用する。
出典・ライセンス・固定コミット・ハッシュは `fixtures/manifest.json`、更新手順は `fixtures/README.md` を参照する。E2Eは `tests/public_pdf_e2e.rs`、混入防止は `tests/test_fixture_hygiene.py` で検証する。外部資料の追加時は出典・利用条件・機密情報の有無と全ページを先に確認する。
