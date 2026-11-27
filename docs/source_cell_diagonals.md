# 原本の対角線をIRとWordセル罫線へ保存する

官報の公開ガイドの四つの斜線は、元の空セルで編集可能なWord対角罫線として保持できるようになった。この追試で検証した範囲は、原本の方向・線幅を持つ中間表現とWordprocessingMLへの保存である。この環境のLibreOffice描画では斜線は表示されない。Microsoft Wordは未導入で実表示を確認していないため、原本の四つの斜線条件を合格へ変更しない。

## 公開Rust APIと適用範囲

`ParsedDocument`へ`source_diagonals: Vec<ParsedSourceDiagonal>`を追加した。型のfieldは`page`、`start: [f32; 2]`、`end: [f32; 2]`、`stroke_width: f32`である。端点は既存graphicsと同じ表示ページ座標系で、方向を外接矩形に潰さない。軸平行の`graphics`配列を変えず、対角線から表の行・列を増やさない。

手作りのIRをstruct literalで作る呼び出し元は、新しいfieldを初期化する。確認済みの原本対角線がないときは次を既存literalへ追加する。

```rust
source_diagonals: Vec::new(),
```

parserが保存するのは、単純な直線subpathを`S`でstrokeし、有限で一様なCTM、正の実効線幅、妥当なline cap、通常合成の不透明なDevice黒、実線、ページ内のstrokeとclip内の中心線矩形を確認できた場合だけである。strokeとfillの状態は別に追跡する。後の不透明な黒い枠は黒線を消す覆いと見なさず、白・灰色・未知の塗り、画像、未確認のOptional Contentなどは候補を保守的に除く。曲線・複数連結線のjoin/miterや非一様変換は単純対角線へ近似しない。Form内の追加抽出と一般vector graphの再現は、この範囲に含めない。

セル境界では原本のstrokeの端がclipされることを許し、中心線の矩形を確認する。f32の変換順序による境界差を最大0.001pt、かつ矩形幅・高さの1/4以下で照合する。保存する端点は変更しない。既存の整数gridへ割り当てる際は1pt以内の対向角一致を使い、原本と文字照合した罫線表の空セルだけを対象にする。横・縦結合は元の所有範囲を使い、継続セルへ線を複製しない。線幅は既存のWord罫線の1/8pt単位へ丸める。原本0.48ptは0.5ptとなり、厳密なink/cap一致は主張しない。

Word側は`w:tl2br`または`w:tr2bl`を使う。[MicrosoftのWord対角罫線の仕様](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376/0ae297ed-7c88-40eb-90a3-62e61a279d5e)に対応する。文字入りセル、短い線、角が一致しない線、破線を空セルの黒い実線へ昇格しない。依存とCLIパラメータの追加はない。

## 固定した検証

親`cfa3d2c0747dcacc979fd5fba9fd564c7ca40f92`、実装`4d64ef6b242f373825ecd6eb2c3bdd3fd49fc33d`。Rust1.93.1のCI defaultは2,763成功・0失敗・42無効化、173テスト群。816個のRust sourceが検証・native/WASM buildを通して一致した。fmt、fixture hygiene2件、WASM distribution10件も通過した。公開parser12件・Word8件、private幾何4件を含み、公開parser/Wordで各32例の移動・一様拡縮PBTを実行した。

欠落するWord罫線、未知の線幅、未確認のcap/変換/連結path、raw NaN座標、セル境界clip、黒い枠の後描画について六つの実際のRedを保存した。各実行の811/814/814/815/815/815個のsourceは固定コミットからbyte一致で再構成できる。初期のcompile失敗も保存し、Red/Greenへ数えない。

初回の全体テストは既存の公開メモリ制限回帰で失敗した。対角線のない入力へ新しいq保存走査が入り、既存64KiB metadata制限より先にForm state深度256を検出した。定数容量の候補走査で不要な追加処理を避け、既存の資源制限条件を保つ。失敗した全体テストは途中1,895成功・1失敗・35無効化であり、合格した全体検証として扱わない。実行815 sourceと失敗ログ・実際のerror種別を保存してから修正し、最終の全体検証を完走した。

既存700結果の成否はすべて親と一致し、699 payloadはRAW一致した。変化は官報native通常の本文XMLへ四つの`w:tr2bl`が加わるだけである。比較のために四要素を外すと本文XMLは親と一致し、他のZIP部品はRAW一致する。実際のpayloadへ正規化は加えない。

新しい評価PDF8件は、二方向、縦結合、セル境界clipと黒枠、短線・文字入り・白い覆い・破線の対照である。事前の入力と期待値を固定し、原本画像を確認した。最初の入力版は白い覆いの後の文字も白になっていたため、Jadeへ一度も渡す前に保留として保存した。別版で文字色を明示的な黒へ戻し、旧版と両goldを変更せず保存した。新版の7画像は目視済み旧版とRGB一致し、変わった1ページを個別に確認した。

native・Chromium・WebKitの通常/strictの追加48結果は、セル文字・所有、2行2列、空セルの斜線方向・線幅・位置、縦結合の条件をすべて満たした。対照は新しい黒い実線への誤昇格を防ぐ条件であり、破線や短線、覆われた外枠の外観を合格とする条件ではない。既知の自作入力であり、未知PDFの正解率にしない。

計748結果はnative244（214成功・30拒否）、両browser各252結果・126入力（216成功・36拒否）となる。両browserの252 payload・126 inspectionは一致した。元のknown11、visibility/baselineの各8入力など、固定した入力群と履歴を保った。

追加8入力では各6出力がRAW一致し、48出力を独立描画した8つのDOCXの8ページへ結び付けた。全ページを個別に確認し、全セル文字は描画にも揃った。官報2ページを合わせ、計10ページを確認した。対角罫線を持つ4つの自作正例と官報の四つの斜線は、この環境のLibreOffice描画では表示されない。

## 原本評価と残る問題

官報は原本ページごとの1,121/501文字、計1,622文字を編集・描画で保持する。52条件は40確認済み合格・9未達・3未確認のままとし、対角線がXMLにあることを表示の合格へ置き換えない。Wordでの実表示、LibreOfficeとの表示互換性、曲線の括弧、原本font metrics、厳密なfooter/heading anchorは残る。

CPI各26/107、労働力調査各36/57、国税庁印紙税額一覧の選択30セルは、RAW一致で前段の評価を継承する。別資料の賞与税率表30条件と混同しない。CPIのgraph・文字切れ・原本12→Word16ページ、労働力調査のvector graph欠落・2→3ページ、印紙税表の印刷ページ番号35/36の欠落は未達である。元の暗号化された官報・印紙税PDFはbrowserで拒否し、native strictも既知の品質損失を拒否する。

85個の既存原本・gold・初回・保留manifest、延べ77,183ファイルを再照合した。自作入力の二版のsource/gold manifest各10ファイルも保持する。外部PDF・font・logoと評価入力はtracked assetへ追加しない。kanpo-parser/kanpo-docxの原本構造に沿う考え方を[固定した参照](source_typography_and_table_opening.md#参照)として使い、custom licenseのコード・依存は取り込んでいない。

hashと検証範囲は[`corpus/validation-source-cell-diagonals.json`](../corpus/validation-source-cell-diagonals.json)へ記録する。`component_qualified=true`はこのIR/OOXML保存の範囲だけであり、`whole_document_accepted=false`、`production_ready=false`を維持する。
