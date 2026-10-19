# OCR runtime と外部モデル

通常CLIは `--ocr` の明示指定で画像だけのページを認識する。
`minimum_pdf_to_docx_with_ocr` example と `jade-pdf2docx-ocr::run_ocr` も利用できる。
root の `ocr-onnx`（crate 単体では `onnx`）を明示して利用する。
`default` / `render` / `ocr` の依存には ONNX Runtime を追加しない。
runtimeのない `ocr` 構成で実画像を推論すると、成功した本文の代わりにエラーを返す。

## モデルの準備と実行

モデル・文字セットは同梱・自動取得しない。[ndlocr-lite-rs](https://github.com/bokuweb/ndlocr-lite-rs) の手順に従い、対応するPARSeq ONNXモデルとNDLmoji YAMLを外部ディレクトリへ用意する。配布元のモデル・文字セット・ONNX Runtimeの利用条件と通知を確認し、必要なライセンス文書を一緒に管理する。以下のパスは手元のファイルへ置き換える。

```sh
cargo run --locked --features ocr-onnx -- \
  input.pdf /tmp/mixed.docx --strict --ocr \
  --ocr-model /absolute/path/to/ocr-assets/parseq.onnx \
  --ocr-charset /absolute/path/to/ocr-assets/NDLmoji.yaml
# 画像ファイルを直接認識する既存example:
cargo run --locked --features ocr-onnx --example minimum_pdf_to_docx_with_ocr -- \
  input.pdf /tmp/ocr.docx \
  --ocr-image /absolute/path/to/line.png \
  --ocr-parseq-model /absolute/path/to/ocr-assets/parseq.onnx \
  --ocr-charset /absolute/path/to/ocr-assets/NDLmoji.yaml \
  --ocr-min-confidence 0.5 --clean-output
```

通常CLIのPDF描画にはPATH上の `pdftoppm`（Poppler）が必要。
`--ocr-dpi` は1〜600、既定150。平均信頼度0.5未満、空本文、描画・推論失敗は
変換失敗として既存出力を保持する。モデルの自動取得やmockへのfallbackは行わない。
OCR対象はparserの `ocr_required` が示すページだけで、native本文・表・画像のある
他ページは既存変換を使い、ページ順を保持する。図版だけのページも対象になる。
ページ外・ゼロ面積・非有限座標の文字はOCR要求を解消しない。
可視Image XObjectを含むページは、native文字が併存してもOCRで本文を確認する。
座標と本文が一致するnative行を元の書式で保持し、未抽出のOCR行を挿入する。
文字の描画モード・完全透明な文字・対応済みclipは抽出へ反映するが、部分文字レイヤの
網羅性や、未知のFont幅・glyph輪郭・画像のclipと透明度を含む完全な可視性は保証しない。
通常CLIは描画したページ全体のRGB各成分が255である場合だけ白紙として保持する。
白紙は本文容量を消費せず、前後の本文とページ境界を保持する。
1成分でも255未満なら白紙扱いせず、行検出できない画像・空の認識結果は失敗する。
ノイズや背景色のあるスキャン白紙の判定は未対応。
Rustでは `jade_pdf2docx_ocr::run_page_ocr` の `OcrPageContent::Blank(VerifiedBlankPage)` と
`convert_minimum_pdf_to_docx_with_bounded_page_ocr_content` でこの区別を渡せる。
callbackは同じPDFの該当ページ全体を検証する責任を持つ。
本文専用の既存 `run_ocr` は白紙を空本文の成功として返さない。
認識本文はページ内の行順と空白を保持するが、縦書き・段組み・表構造の復元は未対応。
`--strict` はOCR以外の品質欠落を推論前に拒否し、必要な全ページの認識成功後だけ
`ocr_required` を解消する。成功は原文との完全一致を保証しない。

通常CLIは最大入力長+1byteまでのコピーで専用領域へPDFを固定し、解析と描画で共有する。
OCR・ページ描画は監督worker内で実行し、既存 `--timeout-ms` と出力公開の保護を使う。
ページ画像は1ページごとに回収する。描画画素数と認識本文の累積容量には上限を設ける。
停止した描画子孫の期限停止と、描画中の親signal/死亡での子孫・一時領域回収は
macOSの実プロセステストで検証した。総RSSの強制制限、単一推論中の停止応答は残課題。

通常CLIは一つの変換内で `PageOcrSession` を共用し、最初の本文認識でモデルと
文字セットをロードする。同じパスの組なら次の行・区間・ページでも再ロードしない。
白紙だけの変換では推論セッションを作らない。モデルと文字セットのパスが変わると
旧セッションを破棄してロードし直し、ロード失敗でも旧モデルを代用しない。
同じパスのファイル内容を差し替える場合は、新しい `PageOcrSession` を作る必要がある。
本文・信頼度・ページごとの本文予算・失敗時ポリシーは単発APIと共通で、
予算超過や画像エラーを次の要求へ持ち越さない。従来の `run_page_ocr` / `run_ocr` は
単発APIとして維持する。

時間内訳が必要な場合は `PageOcrSession::recognize_page_with_timings` を使う。
成功時に本文と `OcrPageTimings`（全体・モデル読込・認識API内・その他処理、読込/認識回数）を返す。
認識API内の時間にはテンソル準備・推論・復号が含まれる。通常APIは計測しない。
手元の画像で初回とセッション再利用を比較する場合は `crates/jade-pdf2docx-ocr/examples/ocr_benchmark.rs` を利用できる。過去のfixture専用取得・配布・評価ツールと保存済み評価値は削除した。現在の公開PDF E2EはOCR精度を測定しない。
