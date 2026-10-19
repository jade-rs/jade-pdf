# `minimum_pdf_to_docx_with_ocr` CLI 仕様

旧OCR fixtureは削除済みです。以下の入力PDF・表JSONは利用者が用意したパスへ置き換えてください。

## 1. 概要

- 対象: `examples/minimum_pdf_to_docx_with_ocr.rs`
- 目的: 最小 PDF -> DOCX 変換に対して、任意で OCR 結果を合成する。

## 2. 実行形式

```bash
cargo run --example minimum_pdf_to_docx_with_ocr -- <input.pdf> <output.docx> [options]
```

### オプション

- 値を取るオプションは `--option value` と `--option=value` の両形式を受け付ける。
- `--ocr-from-pdf` / `--clean-output` のような **フラグ** は値を取らず、指定行のみで有効になる（`--ocr-from-pdf=1` のような形式はエラー）。
- `--ocr-mock <line1|line2|...>`
  - OCR の代替入力行を `|` 区切りで指定する。
  - CJK OCR が文中で分割した行は、DOCX 段落化前に自然な本文行へ連結する。
- `--ocr-image <image_path>`
  - OCR 入力画像（例: ppm）を指定する。本文認識には PARSeq モデルと文字セットが必要。
- `--ocr-from-pdf`
  - 入力 PDF を `pdftoppm`（poppler）で PNG にラスタ化し、ページごとに OCR する。PATH 上に `pdftoppm` が必要。
- `--ocr-auto`
  - PDF 解析結果をページ単位で見て、画像があり、座標付きテキストが 8 文字未満のページだけ `--ocr-from-pdf` 相当のページ OCR を実行する。
  - 判定ロジックは CLI 専用ではなく、`jade_pdf2docx_docx::automatic_ocr_pdf_page_indices` / `jade_pdf2docx_docx::automatic_ocr_pdf_page_indices_with_policy` として library からも利用できる。
- `--ocr-pdf-dpi <dpi>`
  - `--ocr-from-pdf` 時の解像度（既定: `150`）。
- `--clean-output`
  - DOCX からデバッグ用のメタ段落を省略し、OCR で得た本文段落のみを出力する。
- `--ocr-table-json <path>`
  - Phase 1: 外部表パイプラインが出力する JSON（`{ "html": "<table>...</table>" }` 形式）を読み、HTML から `OcrTable` を構築する。
- `--ocr-table-geometry-json <path>`
  - Phase 2: セル bbox とテキスト bbox の JSON を読み、`jade_pdf2docx_ocr::parse_geometry_table_json` で `OcrTable` を構築する（スキーマは `crates/jade-pdf2docx-ocr/src/table_geometry_json.rs` の Rustdoc を参照）。
  - JSON で **`cells` を省略または空配列**にすると、テキスト bbox のみから `ocr_table_from_text_boxes_projection` により行・列を推定する（近似）。
  - 任意フィールド **`cell_assignment`**（既定: `greedy`）で、セルとテキストの対応付けを **貪欲（IoU 最大）** か **Hungarian（最小費用・1 対 1）** に切り替えられる。
  - **`cells` が空** のとき、`texts` のみから **投影**するか、任意フィールド **`lines`**（`[[x,y],[x,y]]` のペア列）から **軸平行罫線で格子**を推定するかを選べる（`lines` 非空なら罫線優先）。
- `--ocr-table-pipeline <executable>`
  - 外部コマンドを `executable + ... + <image>` で実行し、stdout の JSON を表として取り込む。**`--ocr-image` または `--ocr-from-pdf` のどちらかが必須**（`--ocr-from-pdf` のときは先頭ページを PNG にラスタ化して渡す）。
- `--ocr-parseq-model <model.onnx>`
  - PARSeq モデルパスを指定する。
- `--ocr-charset <charset.yaml>`
  - PARSeq 文字セットパスを指定する。
- `--ocr-parseq-failure-policy <fail-fast|fallback>`
  - PARSeq 失敗時の挙動を指定する（既定: `fail-fast`）。
  - `fail-fast`: 認識失敗時に失敗扱い。
  - `fallback`: 信頼度0.0のバンド座標診断を返す。診断は認識本文として取り込めず、exampleは保存前に拒否する。
- `--ocr-min-confidence <0.0..1.0>`
  - PARSeq の最低信頼度閾値を指定する。
- `-h`, `--help`
  - ヘルプを表示して終了する。

## 3. オプション依存ルール

- `--ocr-parseq-model` と `--ocr-charset` は必ず同時指定。
- `--ocr-parseq-failure-policy` は `--ocr-parseq-model` と `--ocr-charset` が前提。
- `--ocr-table-json` / `--ocr-table-geometry-json` / `--ocr-table-pipeline` は同時指定不可。
- `--ocr-table-pipeline` は `--ocr-image` または `--ocr-from-pdf` が必須（コマンドの最後に画像パスを付与する）。

## 4. 入力バリデーション方針

- 未知オプションはエラー。
- オプション値欠落はエラー。
- 同一オプションの重複指定はエラー。
- `--ocr-min-confidence` は `f32` かつ `[0.0, 1.0]` 範囲。
- `--ocr-parseq-failure-policy` は `fail-fast` または `fallback` のみ許可。

### 4.1 代表エラーメッセージ断片

- `usage: cargo run --example minimum_pdf_to_docx_with_ocr -- <input.pdf> <output.docx> [options]`（引数エラー時の stderr に続けて出る usage 行）
- `--ocr-parseq-model and --ocr-charset must be provided together`
- `--ocr-parseq-failure-policy requires --ocr-parseq-model and --ocr-charset`
- `--ocr-min-confidence must be a float`
- `--ocr-min-confidence must be within [0.0, 1.0]`
- `--ocr-pdf-dpi must be a positive integer (dpi for pdftoppm)`
- `does not take a value`（フラグへ `=` で値を付けた場合など）
- `OCR table options are mutually exclusive: --ocr-table-json, --ocr-table-geometry-json, --ocr-table-pipeline`
- `--ocr-table-pipeline requires --ocr-image or --ocr-from-pdf`
- `missing value for option: --ocr-image`
- `duplicate option: --ocr-image`
- `unknown option: --unknown-option`
- `hint: use --help for full options`（引数エラー直後のヒント行）

## 5. OCR ソース優先順位

1. `--ocr-image` が指定され、OCR が成功して非空行を得られた場合はその結果を採用。
2. 1 が失敗または空の場合、`--ocr-from-pdf` が指定されていれば PDF 全ページをラスタ化して OCR。`--ocr-auto` の場合は、画像があり、座標付きテキストが 8 文字未満のページだけをラスタ化して OCR。
3. 2 が失敗または空の場合、`--ocr-mock` があれば失敗理由と代替元を警告し、明示された mock 行へフォールバック。

認識を要求した入力がすべて失敗した場合は終了コード `1` を返し、DOCX を保存しない。診断を含む結果や途中のページの失敗を、一部だけの本文として成功扱いにしない。新規出力を作らず、既存出力を保持する。
現在の `ocr` feature は上流の `onnx` feature を有効化しないため、実モデル推論は未提供。runtime、モデル/文字セットの固定と配布条件、実画像の精度検証は残課題であり、mock/診断テストでは保証しない。

採用された OCR 本文行は、空行を除去した後、CJK 文中の意図しない改行だけを連結する。
句点・閉じ括弧で終わる行や、番号・箇条書きで始まる次行は別段落として保持する。

テキスト情報を十分に持つ PDF ページ内の画像は、図版・写真・ロゴの可能性が高いため `--ocr-auto` では OCR しない。

## 5.1 表（Phase 1: HTML / 外部パイプライン）

- `--ocr-table-json` または `--ocr-table-pipeline` が指定された場合、本文の行 OCR と独立に DOCX へ `<w:tbl>` を追加する。
- モデル未指定の `--ocr-table-pipeline` では画像/PDFを表処理専用入力として扱い、本文OCRを実行しない。
- JSON の `html` は `jade-pdf2docx-docx::ocr_table_from_html` で矩形格子に正規化する。

## 5.2 表（Phase 2: 幾何 JSON）

- `--ocr-table-geometry-json` はテキスト矩形列を必須とし、任意でセル矩形列・列数を含む JSON を受け取る。
- **セルあり**: `jade-pdf2docx-ocr` の `table_match` で IoU 等によりテキストをセルへ割り当てる。
- **セルなし（投影）**: `ocr_table_from_text_boxes_projection` で y 方向に行を分け、**全テキストの x をクラスタして列中心**を決め、各テキストを最近傍列へ割り当てる（行内の単純左詰めより列位置のずれに強い。精度はレイアウトに依存）。

## 6. 出力ポリシー

- `--help` 時は標準出力にヘルプを表示し、終了コード `0`。
- 引数エラー時は標準エラーにエラー内容と usage/hint を表示し、終了コード `2`。
- OCR/変換失敗時は標準エラーに理由を表示し、終了コード `1`。

## 7. 実行例

### 7.1 基本・設定例

```bash
# OCR なし（最小変換）
cargo run --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/minimum.docx
```

```bash
# OCR mock 行を利用
cargo run --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/minimum.docx \
  --ocr-mock "line1|line2"
```

```bash
# OCR 画像 + PARSeq オプション（現在は実推論未提供のため終了1）
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/minimum.docx \
  --ocr-image /path/to/input.ppm \
  --ocr-parseq-model /path/to/model.onnx \
  --ocr-charset /path/to/charset.yaml \
  --ocr-parseq-failure-policy fallback \
  --ocr-min-confidence 0.5
```

```bash
# fail-fast 指定時: image OCR が失敗したら mock にフォールバック
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/minimum.docx \
  --ocr-image /path/to/input.ppm \
  --ocr-mock "MOCK_FALLBACK" \
  --ocr-parseq-model /path/to/missing-model.onnx \
  --ocr-charset /path/to/missing-charset.yaml \
  --ocr-parseq-failure-policy fail-fast
```

```bash
# Phase 1: 外部パイプライン JSON から表のみ（clean output）
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/table.docx \
  --ocr-table-json table_geometry.json \
  --clean-output
```

```bash
# Phase 2: 幾何 JSON（セル bbox + テキスト bbox）から表のみ（clean output）
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/table.docx \
  --ocr-table-geometry-json table_geometry.json \
  --clean-output
```

```bash
# Phase 2: 幾何 JSON + cell_assignment=hungarian（最小費用割当）
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/table.docx \
  --ocr-table-geometry-json table_geometry.json \
  --clean-output
```

```bash
# Phase 2: 罫線 lines のみ（cells 省略・軸平行格子を推定）
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/table.docx \
  --ocr-table-geometry-json table_geometry.json \
  --clean-output
```

```bash
# Phase 2: 罫線 lines のみ（3×3 セル例）
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/table.docx \
  --ocr-table-geometry-json table_geometry.json \
  --clean-output
```

```bash
# Phase 2: テキスト bbox のみ（投影モード・cells 省略）
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/table.docx \
  --ocr-table-geometry-json table_geometry.json \
  --clean-output
```

```bash
# Phase 1: 先頭ページをラスタ化して表パイプラインへ渡す（pdftoppm 必須）
cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/table.docx \
  --ocr-from-pdf \
  --ocr-table-pipeline scripts/table_pipeline_stub.sh \
  --clean-output
```

### 7.2 異常系（期待される失敗）

```bash
# parseq 片側指定（失敗）
cargo run --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/minimum.docx \
  --ocr-parseq-model only-model.onnx
```

```bash
# オプション重複（失敗）
cargo run --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf \
  output/minimum.docx \
  --ocr-image first.ppm \
  --ocr-image second.ppm
```
