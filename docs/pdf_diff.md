# jade-pdf

PDF 用の crate 置き場。

- `jade-pdf-diff` — ページとテキストブロックを対応付けてから比べる、構造を見る PDF の差分ライブラリ。抽出と描画は Pdfium (ネイティブは検証付き取得、WASM は呼び出し側が渡す Emscripten モジュール)。

差分の対応付けには [jade-rs/jade-core](https://github.com/jade-rs/jade-core) の `jade-diff-align` / `jade-diff-support` を使う。

```sh
cargo run -p jade-pdf-diff -- old.pdf new.pdf > diff.json
cargo run -p jade-pdf-diff -- --ir document.pdf
cargo run -p jade-pdf-diff -- --render document.pdf 1 1000 page.png
```

CLI の比較モードの終了コードは `0`: 検出対象の変更なし、`1`: 変更あり、
`2`: 引数・読み込み・解析・実行の失敗（プロセス上限を含む）、`3`: 描画・対応付けの計算上限や未対応の対話要素により要確認。
`3` のときも差分 JSON は出力する。

Rustの `load_document()` / `load_document_blocking()` は、ファイル情報の取得失敗を対象パスの文脈と元の `std::io::Error` とともに返す。`anyhow::Error::downcast_ref::<std::io::Error>()` で `ErrorKind::NotFound` などを確認でき、OSや表示言語の文字列へ依存する必要がない。

各 `PageDiff.text_diff` は、抽出済みIRの順序に沿ったページ本文の変更を返す。行の折り返しを空白として扱い、既存IRと同じNFC・空白・既知の欧文合字等の正規化を適用する。行単位の `blocks`、その文字列・差分区間・位置は保持するため、折り返しによる配置変更と実際の本文編集を別々に表示できる。フォームや描画の差分も引き続き比較する。本文の語順変更は位置の許容誤差で無視しないが、既存のページ番号等の軽微な変更の扱いは保つ。

`text_diff.groups` は変更のある連続範囲で、`old_block_range` / `new_block_range` は各 `PageIr.blocks` の0始まり・終端を含まない位置、`old_blocks` / `new_blocks` は元の `BlockIr.index` への参照である。追加・削除だけなら片側の範囲は空になる。各範囲の正規化済み全文と、両側を再構成できる `segments` を返す。全ページ本文が同じなら空の `groups` を返す。確認済みの軽微なページ番号変更も証拠として含めるが、本文由来のページ状態はそのブロックだけを除いた順序付き全文で決める。本文の繰り返しで一意なアンカーがなくても、この軽微な変更の扱いは変えない。旧JSONのフィールド省略は `text_diff: None` として読み込み、未計算を表す。空の結果とは区別する。これらの連続範囲は、文字の字面に一致するハイライト座標を保証しない。

細かな本文対応付けには共通の前後を除いた書記素対をページ全体で最大4,000,000まで使い、超過すると全文を保持した粗い削除・追加を返す。その場合 `text_diff.alignment_limited` と診断の `page_text_alignment_limited_pages` を記録し、文字比較を要確認にする。段落・表の意味や未知の読み順を復元する機能ではなく、読み順や文字対応の既存の診断も保持する。

CLIは各処理を子プロセスで実行する。既定は子の実行120秒、メモリ2GiB、標準出力・標準エラー・
PNGの合計64MiB。コマンドの前に `--timeout-ms`、`--max-memory-mb`、`--max-output-mb` で変更できる。
上限到達・クラッシュは終了コード2とし、途中のJSONは返さない。PNGは正常終了後に原子的に置き換える。
子の終了時の最大RSSと、実行中のメモリを監視する。macOSは物理フットプリントも参照し、
LinuxはRSSとSwapに加えてRLIMIT_ASを適用する。監視間の増大やOSの実行待ちがあるため、
全環境で厳密なメモリ上限を保証するものではない。UnixではCPU時間・ファイル出力・コアダンプも制限する。
子は親の終了も監視する。これは資源とクラッシュの隔離であり、ファイルやネットワークの権限を制限するサンドボックスではない。

ライブラリからは `isolation::run_pdf_worker()` に配布した `pdf-diff` のパス・CLI引数・
`ProcessLimits` を渡し、終了コード、出力、実測統計を取得できる。
メモリ上のPDFには `isolation::IsolatedPdfClient` を使い、`inspect_bytes()`、`diff_bytes()`、
`text_layer_bytes()`、`render_bytes()`、`render_bytes_at_dpi()` から型付きの結果を取得できる。
比較の `Limits` / `DiffOptions` と表示名を保ち、`value`、`exit_code`、`execution`、`staging` を返す。
`with_process_limits()` と `with_temporary_directory()` で実行上限と一時保存先を指定できる。

バイト入力・通信・PNGは匿名スプールを使う。Unixでは内容を書き込む前にリンク数0を確認し、
Windowsでは最後のハンドルを閉じると削除する一時ファイルを使う。OS別のSDK取得・キャッシュ修復・隔離APIの実行結果は、対象コミットのCIで確認する。
操作用ディレクトリは終了時に明示的に閉じ、後始末の失敗もエラーにする。
入力の一時保存は1ファイルごとの `Limits.max_input_mb` で制限し、子の実行時間とは別に計測する。
通信メタデータは1MiB、受信JSONは深さ256までとし、浮動小数点の正確な往復を有効にして座標を保つ。
PNGはバイナリのまま転送し、メタデータと合わせて出力量の予算に数える。

既存の同期APIは呼び出し元のプロセスで実行し、WASMもこのネイティブ隔離を使わない。
隔離クライアントも同期APIであり、UIでは処理用スレッドから呼び出す。
直接WASMを呼ぶ経路とホスト全体のメモリ・同時実行数の制御は別途必要である。
ブラウザの停止制御には後述のWorker経路を使える。

ライブラリでは `DocumentDiff::has_changes()` と併せて
`DocumentDiff::is_comparison_complete()` を確認する。ファイル・バイト入力では、
テキストに加えて既定144DPIの描画を照合し、画像・図・罫線・文字色・字体の見た目の変更を検出する。
同一バイト列は `comparison.identical_source` に記録して再描画を省略する。
それ以外は、指定した解像度・色差の許容値での比較であり、任意のPDFオブジェクトの完全同値判定ではない。
Unicode抽出の正しさを独立に保証するものでもない。SDKの文字対応エラーは
`PageIr.sdk_unicode_mapping_errors` と `TextRun.sdk_unicode_mapping_errors` に返し、
同一バイト列以外では要確認とする。読める代替文字は保持し、不正な制御文字等は `�` で残す。
既定SDKは、後のToUnicode定義による上書きを修正したPdfium 8111に固定している。
元の文字対応の復元や読み順には引き続き検証すべき範囲がある（[調査記録](../corpus/text-review.json)）。
検出したタグ構造の不整合や、欠けた表セルを座標で補った場合は、
`PageIr.reading_order_requires_review` と文字レイヤーの同名フィールドを返す。
比較診断の `old_pages_with_uncertain_reading_order` / `new_pages_with_uncertain_reading_order` にも集計し、
文字比較の完了判定を要確認にする。異なるバイト列の比較完了も要確認となる。
同一バイト列には従来どおり内容一致の証拠がある。文字・座標・文字幅と、Unicode対応の診断は保持する。
これらのフィールドを省略した旧JSONは、読み順の要確認なしとして読み込める。

小さい上付き・下付きは、文字の原点と字体寸法から幾何的に並べる。8文字以内の括弧付き数値は、代替字体が括弧内へ補った空白で別行扱いにしない。通常の単語間空白・改行は別行の手掛かりとして保持する。これは数式全体の論理構造を保証するものではなく、既存の読み順の要確認は保持する。

水平LTR文字の送りセルが接すると確認できる場合、別オブジェクトの字面の左右余白だけでは単語間空白を生成しない。たとえば細い「1」と次の「8」が連続する `180-3` に、余分な空白が入ることを防ぐ。原文の空白と実際の単語間隔を保持し、文字自体の回転・せん断・不明な送り幅には従来の判定を使う。生成空白の削除と文字セルの更新は読み順の確定後に行う。ただし、送りセルで偽と確認した空白だけを除くと8桁以内の十進数になる場合は、削除前から数値添字として判定する。これにより字面余白の差で添字の「10」が別行へ送られることを防ぐ。実際の単語間空白、文字や演算子を含む未検証の式はこの追加規則へ含めない。文字の字面座標とSDK・原文の診断を保持し、文字列と文字セル・原文領域の添字を一緒に更新する。

全角Han/Kana等の文字同士では、確認できた送りが1文字幅の1%以内で、送り末尾から次の文字までの字間が四分アキ以内の場合に限り、SDK・行結合が補った空白を抑える。Unicode18の割り当て済み文字クラスと原点・送り幅を照合し、濁点の正準分解でも基底文字を同じように扱う。原文とActualTextに書かれた空白、欧文・ハングル、四分アキを超える間隔、未知の幅・回転・せん断は従来の扱いを保つ。削除は読み順の確定後に行い、セルと原文領域の添字を同時に更新する。文字クラス・固定SHA・組版資料と限定範囲は `resources/japanese-tracking.json` に記録する。任意の日本語の語境界やページ全体の読み順を保証するものではない。

SDKのページと入れ子のFormを辿り、文字レコードが1件もないテキストオブジェクトの数を
`PageIr.sdk_text_objects_without_characters` に返す。文字レイヤーでは同名のページ単位の数を各runに返す。
比較診断は `old_pages_with_unobserved_text_objects` / `new_pages_with_unobserved_text_objects` に集計し、
同一バイト列以外の比較と文字比較を要確認にする。空白だけのオブジェクトも含む保守的な判定である。原文から検証したActualTextで表現済みの場合と、SDKの公開マークAPIで空白のActualText指定を厳密に確認できた場合は除く。
文字レイヤーが空でこの診断がある場合、空配列では情報を返せないため明示的なエラーにする。
実際にテキストオブジェクトのない画像ページは空配列を返す。原文のUnicodeや文字位置は推測しない。
探索は文書全体20万オブジェクト・Form32段までとし、超過は部分的な成功を返さずエラーにする。
原文側も別のPDFパーサーで読み、Tj・TJ・引用符の文字表示命令と入れ子のFormから文字コード数の下限を数える。
単純フォントは1バイト、Identity CMapは2バイト、その他の複合CMapは最大4バイトから下限を求める。
ActualTextで置換する描画文字は文字コード下限から除き、SDK側も同じ置換領域の記録を除く。
残る文字と、SDKが挿入した文字を除いた記録数との不足を `source_text_characters_missing` に、
未対応の構文・パターン・解析上限は `source_text_accounting_unavailable` に返す。
これらがある比較は要確認となる。文字レイヤーでは各runにページの診断を付け、空配列で診断を運べない場合は明示的に失敗する。
解析は原文64MiB、ページの展開済み内容2MiB、文書の展開済み内容64MiB、200万作業単位、Form32段まで。
画像描画だけの経路ではこの文字解析を実行しない。
これは文字コード数の下限の照合であり、原文Unicodeや欠落位置の復元ではない。
Unicode展開や別の余剰文字が欠落を覆い隠す場合があるため、不足0も原文の完全抽出の証明にはならない。

さらに、対応できる原文フォントのToUnicode・標準符号化・AdobeのUnicode CMapと、内容ストリームのActualTextからUnicodeの多重集合を作る。
後の定義によるCMap上書き、範囲・配列・複数文字の対応、UTF-16の補助文字を扱い、巨大な文字コード範囲を展開しない。
`source_unicode_scalars_unmatched` は復元後の本文との不一致数、`source_unicode_accounting_unavailable` は原文のUnicode対応を検証できなかった状態を返す。どちらも要確認にする。
一部の対応が不明なときは、確実に読めた原文文字の不足だけを数える。すべて読めたときはSDK側の余剰も数える。
NFD、既知のラテン合字・長いsとアラビア接続形の等価性を用い、空白・制御文字はこのUnicode照合から除く。空白を含む文字コード数の照合は別に残す。
アラビア接続形とその分解先の名目文字に限って互換分解し、上付き数字・丸数字・数学用文字などを一般的なNFKCで同一視しない。
一文字コードのToUnicodeが複数のRTL文字へ展開され、SDKが順番を変えて返す場合は、原文の一意な順序・出現数・厳密な文字コード数と、同じSDKオブジェクト・文字矩形・原点の対応をすべて確認して補正する。英字や数字だけが正順に残る部分的な変化も、原文とSDKの各文字を過不足なく一対一に対応できる場合に復元する。
補正は同じ字形セル内の順序だけを変更し、座標は保持する。英字・数字・結合記号の混在も同じ条件で検証する。
異なる原文順序が同じ文字集合を持つ場合、双方向・結合制御、SDKのUTF-16対を跨ぐ補正、文字欠落、所有関係の不明な場合は `reading_order_requires_review` を保持する。
補助資料はページ当たり1,024候補・64KiB、一つの対応先は256文字までとし、超過は要確認にする。任意の双方向組版の解決を保証するものではない。
Unicode側の予算は文書全体で展開CMap8MiB、400万作業単位、512フォント、各CMap10万区間、対応先1件1024バイトまで。
単純フォントのDifferencesは、コミットとSHA-256を固定したAdobe Glyph List 2.0の4,281名と、AGLの `uni`／`u` 命名規則で照合する。
`union` や `uacute` など、表にある正式名を汎用の接頭辞規則より先に扱う。小文字の16進数、`uni` 内のサロゲート、非標準の別名、不明な複合名は確定した原文Unicodeにしない。
グリフ名は4,096バイト・対応先はUTF-8で1,024バイトまで。名前の読み取りを文書の作業予算に、単純フォント表を8MiBのUnicode対応保存予算に含める。
Windowsの固定SDKが正式な字形名の補助文字を下位16ビットへ切り詰める場合は、原文の使用字形名・使用回数と、同一オブジェクト・矩形・原点のセルを照合して復元する。
原文解析が完全で、欠落・未観測・SDK変換エラー・ActualText置換がなく、重複を除いたセル数が原文コード数と一致し、復元後の文字集合全体が一致することを必要とする。元の値と補助文字がともにUnicode 18.0.0の公式データで明示的な左から右の強い文字（Bidi_Class=L）の場合に限り、通常のBMP文字との衝突や同じ下位値を持つ複数候補は推測しない。
ToUnicodeの明示対応から候補を作らず、正常に復元済みのUTF-16対も補正対象にしない。復元した文字も元のセル座標を保持し、二度の読み取りの生Unicode・座標を指紋で検証する。原文の補助文字資料は1,024種類、RTL補正を含むセル一覧は20万個を上限とする。
未割当の既定値Lは復元の証明に使わない。公式の固定SHA-256・生成方法・Unicode License V3は `resources/unicode-bidi-ltr.json` に記録し、`tools/generate_strong_ltr.py` で再生成できる。追加の実行時依存は不要である。

旧Microsoft Symbolの私用符号は、Type0／Identity-H／CIDFontType2／明示的なCIDToGIDMap Identity、PDF内の三つのSymbol名、埋め込みTrueTypeのfamily／PostScript名、Microsoft Symbol cmapの字形IDとToUnicodeの一致を確認した場合に限り復元する。フォント名だけで私用領域を一括置換しない。Unicode公式Adobe Symbol表から正準合成後も一意な非私用文字だけを採用し、Delta・mu・空白など意味の曖昧な符号は要確認に残す。表の原本通知・固定SHAは `resources/adobe-symbol.json` に記録している。
MT Extraは別の符号化として扱い、同じ検証を通った場合だけF04Bを省略記号U+2026、F06Cをスクリプト小文字l U+2113へ復元する。Symbolの同じ位置はKappa・lambdaを表すため混同しない。MozillaのMathML一次調査・メーカー資料・字体識別情報の出典と対象範囲は `resources/mt-extra.json` に記録する。組み立て用の括弧片・アクセントなど、他の位置は復元しない。
この投影の周辺文字はUnicode 18の明示的なL・EN・ES・ET・CS・NSM・B・S・WS・ONに限定する。引用符や結合記号は元のまま保持し、未割当・私用領域・RTL・Arabic数字・方向制御と、配置処理がRTL候補とする区間を拒否する。例外として、旧Symbolの20種類の括弧組み立て部品は、字体・字形ID・原文符号・ページ全体の使用回数・重複のないセルの所有を確認できた場合だけ、他の既知の記号を復元する文脈に含める。部品自体の私用符号と要確認フラグは保持し、天井・床・括弧全体の論理文字を推測しない。別字体やActualTextとの衝突、未所有の部品は引き続き拒否する。対象と出典は `resources/symbol-component-context.json` に記録する。固定した原本と生成方法は `resources/unicode-legacy-context.json` に記録し、補助文字の字形名復元に必要な強いLの条件は緩めない。原文の全使用回数、ページ全体の文字集合、重複のない一文字セルを確認し、同じ私用符号を他のフォントやActualTextも使う場合は復元しない。検証済みActualText領域の外の字形だけを対象にし、領域内の論理文字は保持する。二度の読み取りで生Unicodeと位置の座標を照合し、空白欠落・未観測オブジェクト・読み順などの診断を消さない。
フォント資料はpure Rustの `ttf-parser = 0.25.1` で名前とcmapだけを読み、外部フォントや輪郭を保存・再利用しない。一つ2MiB、文書全体16MiB（展開失敗も予算を消費）、64表・128名・16cmap表、セル20万個を上限とする。元のToUnicode用の予算は独立して保持する。
未対応の複合CMap、継承CMap、不正なUTF-16などを推測で補完しない。
多重集合の一致も順序・位置・原文全体の完全性の証明ではない。
構造要素のActualTextは、ページ・MCID・Formの名前空間、ParentTreeの逆参照と祖先関係を原文から確認して反映する。
重複MCID、Formの再利用、複数ページにまたがる置換、矛盾する入れ子などは要確認とする。
内容ストリームの非空のActualTextは、原文のMCID祖先・置換文字列・出現回数と、公開SDKマークの同一性を照合して反映する。
同じプロパティ辞書を複数のBDCが参照する場合も、BDCごとの公開ハンドルを区別する。SDKの列挙順を原文の順番へ対応付けない。
固定SDKのマーク共有契約を同梱PDFで実行時に確認し、原文とSDKの対応やForm呼び出しの所有関係が不明なら元の文字と要確認を保持する。
置換文字は対象オブジェクト群全体の領域を所有し、個々の描画文字の位置を保証しない。
文字レイヤーの `source_text_region: true` は原文の置換を含むことを表す。
`source_regions` に、結合後の文字列での `[start, end)` と元の矩形を保持する。添字はUnicode scalar単位で、Rustの `text.chars()`、JSの `Array.from(text)` に対応する。
正の字面がない場合も有限なアンカーを保持し、範囲の `anchor: true` と行の `source_text_anchor: true` で区別する。行全体の高さから文字矩形を推定しない。
アンカー、周囲の本文との混在、複数領域の読み順は要確認とする。空白の置換も単語の区切りとして保持し、空白だけの行は出力しない。
内容ストリームの空のActualTextは、公開マークAPIの生バイトを厳密に復号して元のSDK文字を除く。SDKからの読み取りは1指定4096バイトまでで、超過は元の文字と診断を保持する。
原文のActualText構造は20万作業単位・64段・文字列合計2MiB、SDK投影は文書200万作業単位・Form32段を上限とする。


文字抽出はページの既定座標系で行う。表示回転によってSDKの文字順や合成区切りが変わらないよう、
診断と抽出の間だけページ回転を外し、成功・エラーのどちらでも元の回転へ戻す。
原本の回転、表示寸法、描画、テキストの座標・文字幅はそれぞれのAPIの座標契約を保つ。
回転を外す前に取れていた可視文字とUnicode診断が失われた場合は、部分的なテキストを返さずエラーにする。

横書きの読み順は、文字の配置座標から余白で領域を分け、全幅の見出しと複数段の本文を
別々に並べる。狭い段間を越えて行を結合しない。分割深さは32までに制限する。
同じ字号区分の行を探す際は共通の格子を使い、実際の字号による従来の許容差で候補を確認する。
これにより、12ptと12.045ptのような微小差で、同じ行の所属がページ上の絶対位置に依存して変わる問題を防ぐ。
文字の元座標・幅・SDK診断と、候補判定に使う実測字号は保持する。結合後の字号には、従来どおり行の代表値を使う。
別行・異なる字号区分・広い段間は結合しない。
固定原本の一文字改訂は `python3 tools/audit_scalar_revision.py --binary target/release/pdf-diff` で検証できる。
対象は宣言した文脈内の一文字変更と、独立に確認した文字セルの横位置だけであり、全文や画素単位の強調精度を保証しない。
これは幾何形状による推定であり、タグ付きPDFの論理順序、表のセル順、双方向文字の完全な解決ではない。

斜め文字の連続性は、変換行列を逆に適用した文字空間で判定する。
回転・せん断・縦横で異なる拡大率や負の文字サイズを含む場合も、基線方向と改行方向を区別する。
別オブジェクト間の語間はSDKの文字セルを用いて推定し、既存の文字の矩形・横位置を保持する。
これは斜め文字を含むページ全体の論理的な読み順や、任意の双方向組版の保証ではない。

文字の傾きは変換行列の基線方向から求め、斜体の横せん断と本文の回転を区別する。
同じ基線上の斜体文字を語の途中から外さず、構造情報と位置が一致する表は行・セル順に読む。
実際に傾いた基線や不完全な表の構造は、引き続き読み順の要確認として扱う。
SDKがUTF-16の2要素に分けた文字は、同じテキストオブジェクト・位置・矩形で対応する場合にUnicode文字へ戻す。
原本の重複したToUnicode宣言は保持し、不正・未対応の要素と原本にある置換文字は不確実性を残す。

フォームはWidgetの保存スカラー値、チェック状態、各チェック欄・ラジオボタンの送信値、フィールド属性（読み取り専用・必須など）、選択インデックス・表示ラベルを比較する。
選択欄の `value` は送信値、`display_value` は表示ラベルとして分ける。複数選択では
`value` は最初の送信値で、全選択のラベルは `selected_options` に返す。

表示が古いままの入力値変更も `form_changes` に返す。
文書全体の `canonical_form_changes` ではFieldsツリーの名前付き階層、継承された型・値・属性、
既定値、選択肢と保存された全送信値配列を比較する。Widgetがページにないフィールドも対象になる。
カタログのFieldsにないページWidgetは、親の名前・継承値を予算内で辿って比較する。
一覧が不完全な側に名前が見つからないだけでは、追加・削除と断定しない。
同じ名前で両側に確認できた値の変更は返し、一覧と対話機能の要確認を保持する。
`summary.canonical_form_fields_changed` は文書のフィールド記録の件数で、Widgetの件数とは別に数える。
同じフィールドが両方に現れるため、2つの件数は合算しない。
読み取れない属性は各フィールドの `unavailable_properties` と
`comparison.canonical_form_review_reasons` に残し、確認できる属性の変更は引き続き返す。
未知の値を削除扱いにせず、フィールドの識別に関わるツリーの解析失敗も削除として報告しない。
XFA併用文書もAcroForm側を比較する。テキスト値とJavaScriptのストリームは、無圧縮・
FlateDecode・ASCIIHexDecodeを上限付きで読み、破損や未対応のフィルター等は要確認にする。
XFA自体、未参照の孤立フィールド、アクション全体の意味、署名の検証等は網羅しておらず、
同一バイト列でない文書にフォーム・注釈があれば引き続き要確認を返す。
全Widgetの名前・値・選択肢ラベルは、1文字列200万UTF-16単位と文書の入力サイズ上限で制限する。
Widgetと選択肢の件数は合計10万までとし、読み取りエラーを空値や比較完了として扱わない。
Fieldsツリーの追加解析は入力64MiB、個々の展開ストリーム8MiB、深さ64、走査20万項目、
フィールド10万件と文書ごとの文字・出力予算で制限する。これらはパーサー内部の全割り当てを制限しない。
CLIと隔離APIは追加の子プロセス監視を適用する。
OCR は内蔵していない。外部 OCR の結果は `DocumentIr` として構築し、
`try_diff_documents()` でIR・設定・資源上限を検証してから比較できる。
`diff_documents()` は検証済み入力向けのAPIとして残している。両IR APIはテキスト比較のみで、
画像照合の完了状態は持たない。`is_text_comparison_complete()` は文字抽出・対応付けだけの診断に使う。

描画差分は `pages[].visual` に画素数とPDF座標の変更範囲を返す。
`DiffOptions.visual` の `dpi`、`channel_tolerance`、`max_total_pixels`、`max_source_image_pixels` で制御する。
既定は144DPI・色差0・旧新合計2億画素、1ページの宣言された画像ソース合計6,400万画素。
上限や描画エラーのページは `skipped` と理由を返す。`visual: None` で構造比較だけを実行できる。
位置・文字サイズの許容差は構造比較に適用し、描画比較は画像上の差を独立に判定する。
Rustで `DiffOptions` を直接構築する場合は `visual` を指定するか `..DiffOptions::default()` を使う。

`DiffOptions::validate()` は類似度・重なり率を有限な0〜1、位置・文字サイズの許容差を
有限な0以上に制限する。`diff_files()` / `diff_bytes()` は設定を検証してから読み込む。
`block_match_score` は初回の対応付け、`text_change_score` は残った削除・追加の文字類似度に
よる回収に使う。同じ位置の置換は `position_overlap` による経路でも回収する。
`max_pairwise` を超えた未評価の組は削除・追加のまま残し、
`diagnostics.page_alignment_limited` / `block_alignment_limited_pages` で要確認を報告する。

文書比較の既定上限は512MB・5,000ページ・1ページ20,000ブロック。
ページ数はPdfiumの元の整数値で確認し、65,535ページを超える入力は全APIで拒否する。
パスワード必須のPDFは未対応として理由を返し、空の文書として比較しない。
`diff_files()` には書き込みが完了した入力を渡し、比較中は入力ファイルを変更しない。

テキスト矩形は回転前の PDF ユーザ空間で返す。CropBox の原点は (0, 0) とは
限らないため、描画した `RenderedPage::viewport.pdf_to_pixel` で画像の左上原点の
ピクセル座標へ投影する。`viewport.clipped_bounds(bounds)` は画像外の部分を切り落とす。
変換行列は SVG/Canvas と同じ `[a, b, c, d, e, f]` の順で、回転と実際の画像サイズの
丸めも反映する。描画は両軸 8192px・総画素数 1,600 万以内に制限する。
画像メタデータとネストしたForm XObjectも描画前に検査する。これらの上限は、Pdfium内部の
すべての解凍・解析処理をプロセス単位で制限するものではなく、異常入力の運用検証は継続中。

`PageIr::view_box` と差分の `old_size` / `new_size` は表示領域を保持し、原点だけの
変更も `geometry_changed` になる。旧 OCR JSON では `view_box` を省略できる。
Rust で `PageIr` を直接構築する場合は、既知の表示領域か `view_box: None` と、
`user_unit: Some(1.0)` 等の既知の物理倍率を指定する。旧JSONで倍率を省略した場合は1になる。
座標・文字サイズ・`width` / `height` はPDFユーザ単位のまま保ち、
`PageIr::physical_size_points()` は倍率を掛けた物理寸法を返す（1ポイント=1/72インチ）。
`UserUnit`だけの変更もページ形状変更として検出し、DPI指定の描画と画素予算に倍率を反映する。
位置・フォントサイズ・フォーム矩形の照合は、それぞれのページの倍率を適用した物理ポイントで行う。
座標と倍率を逆に変更して同じ大きさを保った場合、本文の移動や書式変更として誤検出しない。
出力の矩形・文字サイズは元のPDF座標単位を保持する。
位置・文字サイズの許容差は物理ポイントで指定する。幅指定の描画は指定した画素幅を保つ。
倍率の追加解析は入力64MiB、個々の展開ストリーム8MiB、ツリー深さ64、走査20万項目に制限する。
読めない倍率は `user_unit: null` と診断件数に残し、DPI描画をエラー、変更された入力の比較を
要確認にする。幅指定の画像と、その画像へのハイライト投影は引き続き利用できる。

初回起動時の Pdfium のネットワーク取得を避ける配布では `--features bundled-pdfium`
を使う。WASM は呼び出し側で Pdfium の Emscripten モジュールを初期化する。
WASMでは、同梱の `crates/jade-pdf-diff/wasm/pdfium_adapter.mjs` から
`preparePdfiumForJade(pdfium)` を呼び、その後 `initialize_pdfium_render()` に渡す。
固定したpdfium-render 0.8.37の文字矩形の出力引数順と、ActualTextの生バイト取得の関数呼び出し・バッファ長処理を補正するために必要である。
このアダプタと依存版は組にして更新する。試験用のSDKローダーには適用済み。
WASMのPDF処理開始時には、同梱した855バイト・1文字の検査用PDFで、実際のSDKの
文字矩形を確認し、別の固定PDFでActualTextの生バイトも確認する。未補正などで契約が合わなければ、利用者のPDFを開く前に
`PDFIUM_WASM_ABI_MISMATCH` を返す。再初期化でSDKを切り替えた場合も検査する。
ネイティブの既定取得と埋め込みは、同じ配布アーカイブ・ライブラリのSHA-256とサイズを検証する。
キャッシュも使用前にハッシュを確認し、壊れていれば固定アーカイブから原子的に修復する。
`PDFIUM_NO_AUTO_DOWNLOAD=1` ではキャッシュが未取得・破損ならエラーを返し、埋め込み版は内蔵データから復元する。
`JADE_PDFIUM_CACHE_DIR` でキャッシュ先を指定できる（旧 `PDFIUM_AUTO_CACHE_DIR` も受け付ける）。
`PDFIUM_LIB_PATH` は実行時、`PDFIUM_BUNDLE_LIB` はビルド時のカスタムSDK指定で、固定版の検証対象から外れる。
`pdfium_library_path()` で実際に利用するパス、`pdf-diff --licenses` で既定SDK、Adobe Glyph List、Adobe Symbol表、ttf-parser、Unicode資料のライセンス表示を取得できる。ライブラリ配布では `ADOBE_GLYPH_LIST_LICENSE`、`ADOBE_SYMBOL_ENCODING_LICENSE`、`TTF_PARSER_LICENSE`、`UNICODE_DATA_LICENSE` も添付する。
対応する配布物は macOS arm64/x64、Linux glibc arm64/x64、Windows arm64/x64/x86。
WASM 8111用の初期化アダプターと検証付き取得例は `tools/wasm-smoke/prepare_runtime.py` にある。

ブラウザで停止制御が必要な場合は、`crates/jade-pdf-diff/wasm/worker_client.mjs` の
`PdfWorkerClient` と、同じフォルダの `worker_server.mjs` の `servePdfWorker()` を使う。
処理ごとに新しいmodule Workerを作り、
SDKの読み込み・実行をそこで行う。既存の同期WASM APIを直接呼ぶ経路は引き続き同じスレッドで動く。

```js
import { PdfWorkerClient } from '/assets/jade-pdf/worker_client.mjs';
const pdf = new PdfWorkerClient('/pdf-worker.mjs');
const ir = await pdf.run('inspect', [bytes]);
const image = await pdf.run('renderWidth', [bytes], { page: 1, width: 400 });
// image.png: Uint8Array; image.metadata: { width, height, viewport }
const controller = new AbortController();
const job = pdf.run('textLayer', [bytes], { page: 1 }, { signal: controller.signal });
// 必要になった時点で controller.abort()。job は ABORTED で失敗する。
await job;
pdf.dispose();
await pdf.whenIdle();
```

アプリ側の `/pdf-worker.mjs` は、非同期のSDK初期化関数を `servePdfWorker()` へ渡す。
初期化関数は `inspect`・`compare`・`textLayer`・`renderWidth`・`renderDpi` の処理関数を返す。
各関数は入力の `Uint8Array[]` とJSONパラメータを受け取り、前3種はJSON文字列、
描画は `{ png: Uint8Array, metadata: JSON文字列 }` を返す。
実動例は `tools/wasm-smoke/worker_adapter.mjs` と `worker_operations.mjs`。
比較の `Limits` / `DiffOptions` も同じJSONパラメータでRustの検証済み入口へ渡せる。

既定値は開始から120秒、入力1本64MiB・合計128MiB、応答64MiB、同時処理枠1。
パラメータは1MiB・深さ64、応答JSONは深さ256まで。枠が埋まると `BUSY` を返し、キューは作らない。
通常は入力をコピーして保持する。`{ transfer: true }` は入力バッファ全体の所有権をWorkerへ移し、
呼び出し側では失敗・中断時もそのバッファを再利用できなくなる。

時間切れ・キャンセル・破棄では `Worker.terminate()` を要求する。Chromeでは実行中スクリプトの
強制停止が約2秒遅れる場合があるため、中断した枠は既定3秒保持する（`terminationGraceMs`）。
`whenIdle()` は処理とこの枠の解放を待つもので、OSスレッドやヒープの解放完了通知ではない。
期限後の結果は採用しないが、親スレッドの停止やブラウザのタイマー制限を越える厳密な停止時刻、
内部メモリ上限、複数クライアントを含むホスト全体の資源上限は保証しない。
今回の実行検証はChromeに限り、他ブラウザと厳密なメモリ制御は未確認である。

公開 PDF の取得・精度評価・対応範囲は [corpus/README.md](../corpus/README.md) を参照。

プロジェクトのコードは [PolyForm Noncommercial 1.0.0](../LICENSE) に従う。SDKと公開資料は各権利者のライセンスに従う。

空のAcroForm辞書は、Fields配列が空で、使用されない外観既定値だけを持ち、
予算付きの全オブジェクト走査でフィールド・Widget・注釈・動的アクションが見つからない
対応範囲に限り、フォーム由来の要確認を省略する。
`ComparisonCoverage.verified_empty_acroform_shells` は確認できた入力の数を返す。
構造タグのタイトルと属性、線形化情報、Typeを省略したGoTo移動をフィールドと混同しない。
不明なフォーム項目、XFA、署名、孤立・不正フィールド、解析や走査の失敗は要確認として残る。
描画・Unicode・読み順の診断はそれぞれの判定を保持する。

ネイティブSDKの正常なキャッシュは、完全なSHA-256を排他ロック外で確認する。
インストール・破損の修復だけを排他化し、取得後にも再確認してから原子的に配置する。
同じサイズ・更新時刻の破損も検出し、オフラインでは明示的に失敗する。

フォームの有無は、SDKの判定と、ページ寸法の取得時に解析した文書カタログの両方で確認する。
非nullのAcroForm項目があれば型と論理フィールドを調べ、不正な型・循環参照は要確認にする。
フォーム辞書のnull項目と存在しないオブジェクトへの参照は、省略と同じ意味に正規化する。
配列中のnull要素は保持する。無名の子Widgetが親の型や既定値を繰り返しても、
意味が一致する場合だけ同じフィールドの制御と扱い、保存値一覧を重複エラーで失わない。


ブラウザーの直接WASM検証では、Chromeに加えてPlaywrightのFirefox・WebKitを同じ公開PDF・生成条件で実行する。エンジン名とバージョンをレポートへ記録する。

```sh
cd tools/wasm-smoke
npm ci
npx playwright install firefox webkit
cd ../..
JADE_PDF_BROWSER_ENGINE=firefox JADE_PDF_BINARY=target/release/pdf-diff JADE_PDF_BROWSER_REPORT=corpus/results-browser-firefox.json node tools/wasm-smoke/browser_smoke.mjs
JADE_PDF_BROWSER_ENGINE=webkit JADE_PDF_BINARY=target/release/pdf-diff JADE_PDF_BROWSER_REPORT=corpus/results-browser-webkit.json node tools/wasm-smoke/browser_smoke.mjs
```

実行前に公開PDFの取得と `python3 tools/wasm-smoke/prepare_runtime.py --build` が必要である。エンジン指定を省略するとChromiumを使い、不明な名前は明示的に拒否する。`JADE_PDF_BROWSER` は選択したエンジンの実行ファイルを指定する。
直接WASMの精度検証とWorkerの強制終了検証は別の証拠である。Workerの既存全監査はChromium CDPで対象の消滅まで観測するため、Firefox/WebKitを指定した実行では成功扱いにせず拒否する。WebKitの検証はPlaywrightのWebKitビルドであり、Safari製品全体の保証ではない。

Workerの停止回帰では、終了しないWASMにだけジョブ単位の2.5秒期限を指定する。停止後の正常なSDK初期化には通常の既定予算を使い、3秒の初期化遅延を注入しても、同じクライアントで正常に再開できることを確認する。強制停止の期限・退役中の受入制限・CDPによる対象消滅の確認は維持し、復旧の実行有無・経過時間・失敗コードも記録する。

ブラウザーの実行レポートでは、`passed` / `cases` は実行比較と選定した条件の結果である。通常資料に加えて `pending_semantic` の未達資料も実行し、`semantic_gold_checks` / `semantic_gold_passed` / `semantic_gold_failures` へ正誤を別途記録する。`semantic_gold_by_kind` は全ページ文字列と、範囲を限定した文字の出現回数を分けて集計する。既知の誤抽出が両経路で要レビューになっていても、抽出の正解には数えない。未診断の誤抽出や等価書き換えの誤差分は検証を失敗させる。誤抽出を診断できたと数えるには、対象ページ自体に文字欠落・Unicode・読み順の警告、または文字なしの状態が必要であり、別ページや表示寸法だけの警告では代用しない。

goldの `kind` を省略すると `full_page_text` として選択ページ全文をNFCと連続空白だけ揃えて比較する。文字順・結合記号・結合制御・数学文字は保持する。`scalar_inventory` は `ranges` のUnicodeスカラー値ごとに `count_each` の出現回数を、正規化せず照合する。この場合は `scope` が必須で、指定外の本文・順序・座標は検証しない。範囲の重なり・サロゲート・不正値を拒否し、4,096種類・期待総数200万文字を上限とする。ネイティブ監査の `gold_by_kind` と各ブラウザー／Workerの種別集計を併記し、全文の正解率へ文字集合だけの正解を混ぜない。
