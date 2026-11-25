# 登録済みAdobe-Japan1 CIDの復号

国立天文台の2026年暦要項では、非埋め込みType0フォントが`Identity-H`とAdobe/Japan1 Supplement 2を宣言し、ToUnicodeを持たない。旧bytes/WASM経路はCIDをShift_JISへ流していた。元PDFのpage 1から内容・フォントを保って作った既知抜粋では、ChromiumとWebKitのdefault/strict全4出力が同じ文字化けを含むDOCXになった。strictも成功しており、原文の非空白306文字に対し出力298文字、文字の内訳も不一致だった。ネイティブ版の両モードは306文字の内訳を保持した。

入力PDFと出典・利用条件、抜粋前後の検査、PDFKitによる全ページ描画、原文に基づくgoldは、製品修正前にローカル研究領域へ保存した。元の5ページPDFと既存goldは変更していない。抜粋は既知対照であり、未見資料の成績には数えない。

今回の対応は、間接FontオブジェクトのType0/Identity-H、単一CIDFontType0またはCIDFontType2、ToUnicode不在、Adobe/Japan1登録、整数Supplement 0〜7を全て確認した場合に限定する。属性の重複、参照先の世代違い、循環、宣言外の登録名を採用しない。RegistryとOrderingは元PDFの有効revisionから文字列byteを読み、hexとliteral escapeも確認する。構造走査用に文字列を隠した本文から登録名を推測しない。明示ToUnicodeと登録表は混合しない。

UnicodeはAdobe公式の[Mapping Resources for PDF](https://github.com/adobe-type-tools/mapping-resources-pdf/tree/2dd5e53fb74a01718b9dfd448a0d1cce6fff2aa5)の`Adobe-Japan1-UCS2` version 10.002を使う。入力SHA-256は`6a9693361647a37996312cc57071bb79f8c06411207be7c730a83fda1254cd82`。`python3 tools/generate_registered_cids.py`が固定入力の全23,060 CIDを検査し、UTF-8本文とlittle endian offset表を生成する。補助面文字・variation sequenceを保持する。データは静的に借用し、新しいruntime依存や実行時ネットワーク要求はない。出力容量は既存metadata予算に数える。

不完全な二byte対、CID 0、宣言Supplement外のCIDは`ParseError::InvalidRegisteredCid`で解析を中断し、部分本文を公開しない。既に発生した資源上限エラーを後のCIDエラーで置き換えない。登録情報が未確認のFont、Identity-V、独自Encoding、inline Fontはこの対応の対象外で、従来の対応範囲を改善したとは主張しない。

原表および生成データはBSD-3-Clause。`crates/jade-pdf2docx-parser/licenses/adobe-japan1-ucs2.txt`をソース・ネイティブ実行ファイル・WASM配布物に同梱する。WASM生成・配布検査は同一noticeの内容とhashを検査する。lawrenceanumの実装コードは流用していない。

公開API回帰の最初のRedは7件中5件が失敗し、PBTの最小入力は「日」に対応するCID一つだった。最初の接続実装も同じ5件が失敗し、元の登録文字列を読む接続後に7件が通過した。間接Subtypeと正符号付きSupplementの追加Redも別ログで保存した。

全体Rust検証は2,502成功・失敗0・34 ignore（142 groups、750 Rust files）。最終ログのSHA-256は`30e6bc9f2b24f819370ebf24ffd026e20ba3494d59a58d49db46a827d01ea1d8`。配布noticeの検査10件も成功した。ネイティブ132出力（110成功・22拒否）、Chromium/WebKit各140出力（各113成功・27拒否）を実行した。旧128ネイティブ＋272ブラウザーの計400出力は、DOCX 327件と拒否73件のエラーが全て親版とbyte単位で一致した。新しい抜粋は3実行環境×2モードの全6 DOCXが同一で、306文字の内訳も一致する。

元の5ページでは、ネイティブはgoldの2,152文字と一致したが、両ブラウザーは2,153文字となり、元のgoldを**不合格のまま保持**している。差はpage 2のCID `18b5`に対応する`芒`のvariation selector `U+E0100`一つ。元のcontent operand `18b5091b`が一度だけ存在し、固定Adobe原表の`<18b5> <8292db40dd00>`と一致することを、変換後の別監査で確認した。原表はこのsequenceを返すが、元のpdfplumberによるgoldとSDKによるネイティブ抽出はselectorを含まない。この事後監査は新しいprospective goldや未見資料の成績ではなく、元のgoldを修正する理由にも使っていない。

5種類のDOCXを13ページに描画し、全13ページを個別に確認した。ブラウザーの日本語は読めるようになった。一方、ネイティブの抜粋では、旧版で`1 12`だった月日が`112`になり、日曜表でも区切りが減った。非空白文字の内訳一致だけでは検出できない退行である。`pdfium_reconcile`は非空白件数が一致したページでpure側の空白配置を採用するため、登録CIDの正しい復号によってこの経路が新たに成立することが原因候補である。原文数値の書体差、数値の折り返し・clipping、表の位置、月相図形の欠落も残る。

比較結果は`corpus/validation-registered-japan1-cid-held.json`に記録した。元の44基準の全評価は未完了である。**この変更はHELD／未適格で、PR未作成**。次の修正ではnativeの論理的な空白とvariation sequenceを検証し、元PDF・元gold・今回の比較結果は変更しない。日本語組版・表・太字・縦書き・ページ再現の合格は主張しない。
