# 明示対応を持つsymbolic Mac fontの再利用

所有する4ページのPDFでは、埋め込み許可・字形・ToUnicodeがそろっていても、Mac platformのformat6 cmapだけを持つsubset fontが再利用されず、表内の矩形字形と9/12ptが代替fontの12ptへ変わっていた。この既知の不具合を、修正前のテストで「font0件、期待1件」と再現した。

[Adobe PDF Reference 1.7 §5.5.5](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.7old.pdf)と[ISO 32000-1 §9.6.6.4](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf)は、Encoding未指定またはSymbolic指定のTrueTypeについて、Mac(1,0) cmapを使って単byteから字形を選ぶ経路を定めている。[OpenType cmap仕様](https://learn.microsoft.com/en-us/typography/opentype/spec/cmap)ではMac platformとUnicode platformを区別する。このため、PDFのbyte codeから元のglyph ID、明示ToUnicodeからUnicode scalarをそれぞれ確認し、二つを合成する。

既存の埋め込み許可を通過し、Unicode subset検査で従来はfallbackになっていた候補だけに適用する。世代0の一つのTrueType resourceとdescriptor、Symbolicフラグ、Encoding未指定、重要属性の重複がない完成辞書を確認する。ToUnicode全体は許可code space内の単byte・単一BMP scalarに限り、未対応文字対応の一部分だけを採用しない。異なるglyph IDを同じUnicodeへ対応させる曖昧さは、.notdefとの競合も含めて拒否する。

fontは単一Mac(1,0)・言語0のformat6、byte範囲、SFNT directoryの一意性・境界・重なり・各表と全体のchecksumを検査する。複雑なshaping、variation、color、bitmap、署名などを持つ候補は対象外。PDFのFirstChar/LastCharと直接のWidths配列を確認し、対応する実字形の幅がhmtxと0.001/1000em以内で一致することを求める。PDF realの十進表記を使い、f64 parserが受理する指数表記を受け入れない。

Unicode cmapとheadのchecksum adjustmentを更新し、元のglyph・metrics・name・権利情報・hintなどの各表は維持する。既存の累積font保存予算を使い、停止時は部分fontを公開しない。候補には元descriptor番号を保持するため、metadata容量の独立した境界モデルも更新した。依存、公開API、CLI、tracked PDF/font fixtureの追加はない。

9テストは、元の回帰、PDF幅の指数表記、.notdefの曖昧な対応、Font種別・flag・共有参照、Widths、ToUnicode全体、cmapと破損program、保存予算、byte codeとUnicodeを別々に変える32例のpropertyを検査する。parser全体は1,105成功・失敗0・27ignored。実際の三つのRedと対応するsource archiveを保持する。

既知の所有PDFを通常・strictで追試し、同一のDOCXへ一致した。4ページすべてで原本2×2セル、96字、9/12ptを復元する。埋め込まれた二つのfontは、cmapとhead adjustment以外の表全バイトが原本と一致する。現在の4ページPNGも、個別に目視済みのUnicode controlとRAW一致する。これは新たなinternet PDFやgenuine unseenの精度値ではない。

元のbaseline許容誤差0.05ptは維持する。符号付き誤差は+1.406〜+2.506ptで未達であり、選択とfont再利用の修正で解消したことにはしない。Microsoft Word表示は未検証。全体検査は2,809成功・失敗0・42ignored、177グループ。829個のRustファイルは全検査・native/WASM build・772結果の追試の前後で一致する。別のNTA向けglyph・字幅・背景実装はこのsnapshotへ含めない。


native252、Chromium260、WebKit260の合計772結果は、666成功出力・106想定どおりの拒否。既存760結果とUnicode control6出力はRAW一致する。変わったMac control6出力は全経路・通常/strictで同一DOCXへ一致し、原本セル・字形・サイズを保持する。両ブラウザの解析結果はこのMac controlの行数4→2、非空セル8→4だけが変わる。これらの実行状態は精度率として扱わない。

原本90 manifestの94,063項目・93,285個の一意なファイル、NTA原本16ファイル、二つの所有原本セットを再hashした。元の0.05ptを含む条件、最初のMac font欠落、NTAのgenuine unseen6/62を再ラベルしない。官報40/52、CPI26/107、労働力調査36/57などの既存の未達も維持する。

検証のhashと範囲は[`corpus/validation-verified-symbolic-mac-fonts.json`](../corpus/validation-verified-symbolic-mac-fonts.json)へ記録する。`component_qualified=true`は確認済みのMac font再利用の範囲であり、`whole_document_accepted=false`、`production_ready=false`を維持する。
