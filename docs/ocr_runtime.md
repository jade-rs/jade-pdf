# OCR runtime と外部モデル

通常CLIは `--ocr` の明示指定で、画像本文の確認が必要なページ・領域を認識する。
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
全ページOCRでも、座標付き認識行はページ基準の独立したテキストframeへ保持する。
行を座標順へ並べ替えず、認識器が返した読み順と空白を維持する。
座標なしcallbackは従来の段落出力を使う。OCRから元のfont・装飾を復元するものではない。
認識本文はページ内の行順と空白を保持するが、縦書き・段組み・表構造の復元は未対応。
Wordの各辺22in上限を超えるページは、OCR枠・文字・表・画像・余白を出力時に等比縮小する。
原寸の物理サイズが変わるため `page_resized_for_word` を報告し、`--strict` では推論前に拒否する。
原本PDF・描画DPI・callbackの原寸座標・読み順・認識本文は変更しない。
`--strict` はOCR以外の品質欠落を推論前に拒否し、必要な全ページの認識成功後だけ
`ocr_required` を解消する。成功は原文との完全一致を保証しない。

ページ本文で描画された曲線pathは、罫線IRで再現できないためページごとに
`unsupported_curved_path` を報告する。曲線から文字が描かれている場合もあり、
既存の文字レイヤが残っていても `--strict` はこの欠落を拒否する。
Rustでは `QualityIssue::UnsupportedCurvedPath { page }`、WASMのinspectionでは
同じkindと0始まりのpageを返す。描画を取り消したpathや文字列内の偽命令は対象外。
これはOCRの認識成功を意味せず、現在の画像対象OCRを自動起動するものでもない。
曲線を含むpathを、存在しない表の直線罫線へ近似することも避ける。
未診断のベクトル描画や複雑なclipの完全な再現を保証するものではない。

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

埋め込みType1 FontFileでPDF側BaseEncodingがない場合、対応済みの静的な組み込みEncodingとDifferencesを適用する。ToUnicodeはcodeごとに優先する。実際に描画したglyph名が確認不能なら `UnsupportedFontReason::Encoding` / `unverified_encoding` を報告し、strictは拒否する。未使用の未知slotだけでは拒否しない。動的PostScriptやISOLatin1基底表は未検証で、独自glyph名のOCR復元を意味しない。

完全な描画用Type1の検証には、既存の公開fixtureからToUnicodeだけを外した一時入力を使う。`python3 tools/generate_type1_drawing_controls.py /tmp/jade-type1-controls-UNIQUE`（pypdfが必要）で未使用のディレクトリへ生成し、利用条件・全ページの描画不変性を確認してから `JADE_TYPE1_DRAWING_CONTROLS=/tmp/jade-type1-controls-UNIQUE cargo test -p jade-pdf2docx-parser --test external_type1_drawing -- --ignored` を実行する。元PDF・正解本文・FontFileは変更しない。Type1の既知のFontDirectory/UniqueID/FontType比較によるcache前処理は有限の字句列として認識し、PostScriptを実行しない。未知の実行式は確認不能のまま扱う。

標準14書体以外のType1で組み込みfont programを取得できず、使用codeのToUnicodeもなければ確認不能としてstrictで拒否する。任意のEncodingのnull・未定義参照はPDF 32000-1:2008 §7.3.7/7.3.9/7.3.10に従い省略と同じ扱いにして、利用できる組み込み表を使う。標準14書体の非埋め込み経路は維持する。

同じTJ配列内で語間を持つ文字同士が0.2em以上離れる場合は、glyph座標を変えず空白を一つ補う。明示空白・小さいkerning・CJKのtracking・別の描画命令には追加しない。空白を除いた文字列一致だけでは単語欠落を検出できないため、実フォント検証では単語列も比較する。フォント代替や実際の改ページの一致は別途描画して確認する。

native抽出でPdfiumが印刷された行末hyphenをU+0002へ置換した場合は、SDKのIsHyphen判定がある文字だけを元のhyphenへ戻す。通常segment・ページpreview・不可視文字を除く経路で同じ判定を使い、文字数や順序が合わない範囲を推測で置換しない。原文の単語区切りと句読点をE2Eでも保持し、正解本文のhyphenを削除する比較は行わない。

組み込み表を取得できず追加のglyph名対応もない場合、既存ToUnicodeを複製せず共有し、未確認codeのbit列だけを診断で保持する。通常出力には未検証のfallback文字が残る場合があり、本文復元の成功を意味しない。strictは描画した未確認codeを拒否する。
