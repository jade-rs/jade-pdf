# 確認済みの原本セルを表選択で維持する

完成した罫線表の一セルに複数の表示行があると、文字だけから推測した表の非空セル数が多くなる。従来は、この数の差だけで2列2行の原本表を2列4行へ置き換え、元のセルに対応した埋め込みfontと文字サイズを失う場合があった。

全非空セルで本文の対応、行の容量、再利用可能なfontを確認できた原本表は、この置換から保護する。既存の小数罫線寸法で使う確認処理を共有し、段落を生成せずに判定する。空の表、セル対応が欠けた表、fontを確認できない表は従来の推測経路を使う。結合セルの既存の保護も維持する。

この変更は[原本セル内のタブ位置](source_cell_tab_stops.md)の検証済みsnapshotを基点とし、別途検証中のNTA向けglyph情報・字幅font変換・背景frameの変更を含まない。[kanpo-parser / kanpo-docxの参照記録](source_typography_and_table_opening.md#参照)で整理した、原本gridとセル所有を確認して書式を保持する考え方に沿う独立実装である。

所有する矩形fontで、2行を4行へ置換してしまう失敗を修正前に再現した。修正後は同じ回帰、font欠落、セル欠落、32例の平行移動の4テストが成功する。テストfontは既存の所有font writerから作り、全SFNT checksumと独立readerの字形・advanceを検証する。外部PDFやfontをtracked fixtureへ追加しない。

表の選択と書式の保持は、原本のbaselineや字面が正しく表示されることまで保証しない。所有する4ページのUnicode font付きPDFでは、修正前に4行へ分割される不具合を確認しているが、baselineの元の許容誤差0.05ptを緩めず、表示の確認は別に扱う。Mac cmapだけを持つ未対応のsubset fontも合格へ変更しない。Microsoft Wordでの検証と文書全体のproduction readinessは未達のままである。

全体検査はRust 2,800成功・失敗0・42ignored、177グループ。固定した827個のRustファイルを検査・native/WASM build・追試の前後で照合した。既存90個の原本gold manifestの94,063項目と93,285個の一意なファイル、NTAの原本16ファイル、所有する二つの原本セットを再hashし、元の条件を維持した。

native252、Chromium260、WebKit260の合計772結果は、666出力成功・106想定どおりの拒否。既存760結果は出力またはエラーまでRAW一致し、両ブラウザの既存128件の解析結果も一致した。Mac format6の負例6出力もRAW一致する。変わったのはUnicode font付き所有PDFの6出力だけで、全経路・通常/strictが一つの同じDOCXへ一致した。2列2行のセル所有、96字の矩形字形、9/12pt、元の二つのfont program全バイトを保持する。

現在のnative出力を再描画した全4ページも、個別に目視済みの試作4ページとPNGがRAW一致する。baselineの符号付き誤差は+1.406〜+2.506ptで、原本の0.05pt条件は未達のまま。表示を全面合格へ変更しない。NTAの最初の6/62、官報40/52、CPI26/107、労働力調査36/57も既存出力とともに維持する。この小さい変更へ、未検証のNTA向け実装を含めない。

hashと範囲は[`corpus/validation-verified-source-grid-selection.json`](../corpus/validation-verified-source-grid-selection.json)に記録する。`component_qualified=true`はこの限定した表選択の範囲であり、`whole_document_accepted=false`、`production_ready=false`を維持する。
