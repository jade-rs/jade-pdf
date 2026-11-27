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

## Word出力用の画像抽出

`parse_pdf_for_word` と `parse_pdf_bytes_for_word` は、可視本文のあるページで、他の描画より先に紙全域へ描いた完全に白く不透明な画像を省きます。ページの描画順と色を確認できた画像に適用し、通常の `parse_pdf_with_options` と `parse_pdf_bytes_with_options` では画像も抽出します。

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

## LZW本文とContentsの解決

正式なpage treeの`Contents`は、間接stream・直接配列・間接配列を解決し、参照世代・循環・型を検査します。配列の同一streamを省略せず、元の順序とstream間の描画状態を保ちます。空配列・null・欠落は空白ページとして扱います。実行回数はxrefの一意entry数とは別に、metadata・確保・累積展開の既存予算で制限します。

直接`/Filter /LZWDecode`はpure Rustで9〜12bitを復号します。直接`DecodeParms`の`EarlyChange`は0/1（既定1）、`Predictor`は1（無予測）のみ対応します。作業辞書と展開配列は共通allocation予算へ事前予約し、単一・累積展開量と協調キャンセルも適用します。破損LZWは`ParseError::Io`の`InvalidData`、未対応LZW parameterは`Unsupported`として返り、部分本文や空白文書を成功扱いしません。

Filterの配列・間接Filter/DecodeParms・LZW予測変換には未対応です。正式な`Contents`を持たないRootなし互換入力の探索や、他のcodecにおける復号不能時の診断は今回の対応範囲外です。画像だけのPDFは画像を保存してもeditable文字にはならず、OCR診断とstrict拒否は引き続き適用します。新しい公開PDFはfixtureへ追加せず、出典・権利・機密情報・全ページを確認してから、一時領域で画素・出現回数・順序と全出力ページを検証します。
