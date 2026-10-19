# 開発ガイド

[READMEへ戻る](../README.md)

## 構成

| package | 役割 |
| --- | --- |
| `jade-pdf-diff` | PDF差分のライブラリとCLI |
| `jade-pdf2docx` | PDF → DOCXのCLI |
| `jade-pdf2docx-parser` | PDF解析と中間表現への変換 |
| `jade-pdf2docx-docx` | 中間表現からDOCXへの変換 |
| `jade-pdf2docx-ocr` | optionalなOCR連携 |
| `jade-pdf2docx-ocr-correction` | OCR結果の補正候補生成 |
| `jade-pdf2docx-pdf-raster` | PDFの描画 |
| `jade-pdf2docx-wasm` | ブラウザ向け変換API |

開発方針は [AGENTS.md](../AGENTS.md) を参照してください。

## ビルド・テスト

Rust stable（rustfmt・Clippy）とPython 3.11以上を使用します。以下はリポジトリのルートで実行します。

```sh
cargo build -p jade-pdf2docx --locked
cargo fmt --all -- --check
RUST_TEST_THREADS=1 sh scripts/check_rust_features.sh default
python3 -m unittest discover -s tests -p 'test_*.py'
```

feature検証の共通scriptは、差分crateを除いたworkspaceのClippyとテストを順に実行します。追加featureは `default` を `render`、`ocr`、`ocr-onnx` に置き換えて検証できます。実OCRの準備は [OCR runtime](ocr_runtime.md)、差分crateの検証は [PDF差分ガイド](pdf_diff.md) を参照してください。

WASMのビルド確認:

```sh
rustup target add wasm32-unknown-unknown
cargo check -p jade-pdf2docx-wasm --target wasm32-unknown-unknown --locked
```

## テストデータ

公開PDF 3件によるCLI E2Eと、入力を生成する単体・プロパティテストを使用します。出典・ライセンス・更新手順は [fixtureガイド](../fixtures/README.md) を参照してください。

```sh
cargo test -p jade-pdf2docx --test public_pdf_e2e --locked
python3 -m unittest discover -s tests -p test_fixture_hygiene.py
```

## ブラウザplayground

RustのWASM target、wasm-pack、Node.jsとnpmを用意します。

```sh
rustup target add wasm32-unknown-unknown
cd playground
npm ci
npm run build
npm run dev
```

ビルド時に `jade-pdf2docx-wasm` からブラウザ向けAPIを生成します。
