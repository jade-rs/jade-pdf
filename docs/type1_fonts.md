# Type1 font の対応範囲と検証

PDF の Type1 `/FontFile` に含まれる対応済みの静的描画プログラムを、pure Rust で OpenType/CFF に変換し、DOCX に埋め込みます。`parse_pdf_bytes` と bytes/WASM 変換で利用でき、native のファイル変換でも文字照合が成立するページでは選択した font を保持します。新しい CLI オプションや外部 font インストールは不要です。

元の PDF code、Encoding/Differences、ToUnicode、Widths を対応付けます。同じ source glyph に異なる幅が指定されている場合や、複数 Unicode 文字に対応する ligature では、必要な face を分けます。通常の `f` の連続を元の `ff` glyph に誤変換しないよう、ligature 用 face の適用範囲を元の描画 code に限定します。`Tc`、`Tw`、`Tz`、`TJ` の位置計算は元の code 幅に従います。Form 内の同名 resource は、ページ側の同名 font と区別します。

変換した family 名は `JadeT1-` で始まる SHA-256 由来の識別名です。元 program、幅、文字対応、属性、権利表示、実際の glyph 出力を識別に含め、元の予約 family 名を再利用しません。PDF の名前の部分文字列から bold/italic を推定せず、生成 SFNT の属性を DOCX run と埋め込み slot に反映します。

## 元glyphと空白の位置

生成faceを選んだ正立文字は、同じfontでも元PDFのcodeごとに `ParsedTextElement` を分けます。一つの元合字に対応する `ff` は一要素、二つの元 `f` は二要素です。呼び出し側は、一要素が単語や行全体を表すと仮定せず、要素の順序・矩形・本文を扱ってください。正立した描画行列からbaselineを確認できた要素では、pure/bytes/WASMでも `baseline_y` を設定します。未対応の回転・反転からbaselineを推測せず、従来の同一face断片の結合を保持します。previewは正立glyphを有界な行へ戻してから重複除去し、反復した文字や論理空白を失いません。

`baseline_y` は、確認済み水平書字の正立描画では通常のsimple fontと対応済みCID fontにも設定します。詳細は[文字基線の契約](text_baselines.md)を参照してください。派生Type1のglyph単位の分割条件とfont許諾は維持します。

`TJ`から復元した論理空白は、両端の正立glyphと同じbaseline・em高を確認できたとき、`font_name: None` の独立した矩形として保持します。元fontのcode32や未知glyphへU+0020を割り当てる処理ではありません。

DOCXでは、行全体について一つの元glyphのadvanceをcmap/hmtxまたは対応済みrequired rligから確認できたときだけ、整数percentの文字幅とtwip単位の字間を指定します。各次原点から字間を計算し、数値計画上の丸め誤差の累積を抑えます。小さい負のkerningは字形の圧縮で代用しません。複雑なshaping、未知字形、未確認位置を含む行には、この補正を部分適用しません。

論理空白にはU+0020だけを持つ所有TrueType fontを文書内に生成し、1ptで選択します。埋め込み有無を問わずsource familyとの衝突を避け、元programを改変しません。header/footerだけで参照するfontも梱包します。この内部fontを使うDOCXでは、明示した空白幅を半emへ変更する `balanceSingleByteDoubleByteWidth` 互換設定を外します。他の互換設定は保持します。内部fontのname tableには本リポジトリの著作権・ライセンス本文を保持します。

half-pointのfont size、整数percent、描画器側の丸めには制約があります。数値計画のproperty testは、Microsoft WordやLibreOfficeで全原点が同じ精度になることを保証しません。初行baseline、混在font、ページ送りも引き続き実描画で検証する必要があります。

## 権利と制限

実際に埋め込むfaceがない場合だけ、既知の `NimbusRomNo9L-Regu` / `Medi` / `ReguItal` / `MediItal` を
`Times New Roman` familyへ指定し、対応するbold/italic属性を保持する。
6文字大文字のsubset prefixを除く既知のface名だけが対象で、部分文字列や未知の変形名から推測しない。
同じ代替familyになる表の混在faceも、一つの共通styleへ潰さない。埋め込みfaceの名前と内部styleは保持する。
これは描画環境が持つ代替fontの選択であり、新しいfont programの生成や埋め込み許可の根拠ではない。
名前の対応はArtifexの[Fontmap.OS2](https://github.com/ArtifexSoftware/ghostpdl/blob/master/lib/Fontmap.OS2)で照合した。

段組みと代替familyを併用した現在の検証は[段組みガイド](text_columns.md)と
[検証台帳](../corpus/validation-narrow-text-columns.json)を参照する。
GNU原本17ページに対し段組み修正時はnative18・pure17ページまで減った。次節のCFF実数表記の修正で、
この資料の確認したCMTT文字は実描画でも表示された。ページ数だけによる合格判定はしない。
その後の[native文字番号による抽出](native_text.md)で、矩形抽出の重複によるnativeの1ページ増加も解消した。
以下のPR #29・#30時点の測定値は当時の履歴であり、現在の全体精度を表すものではない。

## CFF実数表記と描画互換性

生成するCFFの実数は、整数の仮数と補正した指数で表す。たとえば
`6.112469437652811e-5` は `6112469437652811e-20` とする。
有効数字を切り捨てず、数値・元glyphの描画命令・cmap・advance・em単位数を保持する。
固定容量のバッファを使い、追加依存やfontのインストールは不要である。

GNU Helloの二つのCMTT faceでは、修正前の生成fontを同梱LibreOffice
26.8.0.0.alpha0で描画すると、出力PDF内のType1 FontMatrixが意図した値の約1/10になった。
整数仮数への変更後は約1倍になり、索引で確認した7語の描画枠に実際の黒画素が現れた。
このCFF表記だけを修正した時点ではnativeは18ページ、pure/WASMは17ページで、修正前のページ数を維持した。
これは抽出文字列だけの検査ではなく、実際の製品DOCXから描画したPDFと画像の検査である。

数値の表現は[Adobe CFF仕様のDICT実数形式](https://adobe-type-tools.github.io/font-tech-notes/pdfs/5176.CFF.pdf)に従う。
観測した桁ずれは、公開されている[LibreOfficeのreadRealVal](https://docs.libreoffice.org/vcl/html/cff_8cxx_source.html)
で小数点の後の指数桁も小数桁に数える処理から説明できる。製品にはその実装を転用せず、
同値な十進表現を生成する小さなRust処理を加えた。

対応するem単位数16〜16384の全16,369値と、有限f64のbit patternをサンプリングするproperty testで
値・符号を検証した。property testは全f64の総当たりではない。
実資料11件・Chromium/WebKit・default/strictの44結果で、診断と拒否動作は同一だった。
全出力の本文・段組み・画像を含むfont以外のZIP部品は同一である。
生成fontのCFF表記とchecksumは意図して変わるが、独立したfontTools検査でglyph命令・
cmap・hmtx・em単位数・権利表示などを保持した。

検証の詳細は[台帳](../corpus/validation-cff-real-rendering.json)を参照する。
この検証時には論文の過剰なページ数、数式・図・表、未確認Unicode、GNU nativeの1ページ増加が残った。
nativeのページ増加は別の文字抽出修正で解消したが、未確認Unicodeなどは引き続き残る。
Microsoft Wordや未見資料を含む本番品質の認定ではない。

変換前に元 program の FSType と確認可能なライセンス表示を検査します。対応する OFL 1.1 表示の場合は元の著作権・notice と標準ライセンス本文を保持します。埋め込みが許されない場合や権利根拠を確認できない場合は、この変換経路では font を生成しません。no-subsetting 属性を保持し、元 program 内の全 glyph を出力へ残します。これは元の Type1 暗号文や hint 命令を同一 byte のまま保存することを意味しません。

対応は、検証済みの binary eexec、静的 CharStrings/Subrs、標準的な FontMatrix などに限定します。任意の PostScript、任意の FontMatrix、あらゆる複合文字や shaping を実装したわけではありません。元 glyph の Unicode が未確認なら、別の Unicode や PUA を推測して割り当てません。既存の `unverified_encoding` 診断と strict の拒否は維持します。

中間 font、文字属性、resource cache、埋め込み byte 数は既存の解析予算で制限します。未使用 resource の face は公開結果へ含めず、最終結果の埋め込み font 数上限は既存の32件です。native の文字数不一致や Form の抽出順不一致はページ単位で SDK 結果へ戻します。生成 cmap と異なる Unicode に置き換わる場合も、そのページへ元の生成 font を引き継ぎません。

## 検証と残っている問題

位置保持の検証は[検証台帳](../corpus/validation-type1-word-spacing.json)に記録しています。公開3原本・全6ページの単語順と行末を独立に照合し、WASMの4ページ原本は従来の8ページから4ページへ戻りました。nativeとWASMの実DOCXも一致します。Type1原本の横位置の最大差は約1.335pt、baselineは最大4.38pt残ります。GNU原本17ページはnative26ページ・pure/WASM25ページで、引き続き不合格です。通常CI相当2,218件、Python152件、Chromium/WebKit各72件が成功し、実6入力・2engineのdefault/strict合計24結果も保持しました。これらは本番品質の認定や未見資料の精度ではありません。

所有するメモリ内 PDF による Red/Green テストで、混在した単文字・複数文字 code、literal/hex/octal、`TJ` と文字間隔、同名 resource の異なる幅、Form の局所 scope、文書間 cache reset、未使用 font、native のページ単位 fallback を検証しています。font metadata の境界、style flags、ページ単位の不変条件には property test もあります。

公開資料の回帰検証には [GNU Hello manual](https://www.gnu.org/software/hello/manual/hello.pdf) を使用しました。原本 SHA-256 は `3c5212427682a49771d653a5f46ae2e69bdee33b4c0cf428d6ec672ab99d5e3b`、17ページです。原本の GFDL 1.3+ 表示と埋め込み Computer Modern font の OFL 表示を確認し、変換前に全ページを確認しています。原本と出力は Git に追加していません。この資料は改善に用いた既知の回帰資料であり、未見の holdout ではありません。

PR #29の検証では11個の元 font から15 face を生成し、独立した fontTools/HarfBuzz 検査で339個の確認済み source code を機能の有効・無効の両方で照合しました。678件の照合は font shaping の検査数であり、PDF 件数ではありません。当時の純 Rust の実解析では861要素のうち859要素、nativeでは843要素のうち794要素が生成 face を参照しました。これらはglyphごとの要素分割を導入する前の件数です。一ページの文字数不一致は他ページの選択を失わせません。

PR #29時点の native CLI 出力には15 face が入り、同梱 LibreOffice で描画した PDF にも生成 family が現れました。しかし、17ページの原本が26ページに増え、索引の配置も崩れています。Unicode 未確認の `circlecopyrt`、native の一ページの文字数差、空白 glyph の代替、折り返しとページ送りは未解決です。Microsoft Word での描画は未検証です。font の描画・幅・Unicode の部品検査が通っても、文書全体の本番品質を保証しません。

SFNT 属性は [OpenType OS/2](https://learn.microsoft.com/en-us/typography/opentype/spec/os2) と [head](https://learn.microsoft.com/en-us/typography/opentype/spec/head)、DOCX の face slot は [Microsoft の font 実装注記](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/09b83e15-30de-481d-8ddf-777107d4122e) を参照しています。

空白の互換補正は [MicrosoftのbalanceSingleByteDoubleByteWidth仕様](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.balancesinglebytedoublebytewidth?view=openxml-3.0.1) を参照しています。advance読み取りに使うpure Rustの `ttf-parser = 0.25.1` の[MIT notice](../crates/jade-pdf2docx-docx/licenses/ttf-parser-0.25.1.txt)を、native/WASMの再配布にも含めてください。
