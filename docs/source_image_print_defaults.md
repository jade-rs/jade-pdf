# 通常の印刷設定と画像の画素

画像へ作用しないことを確認したExtGStateの通常設定を、未知の画像効果と区別する。画像の配置、グラフの構造、ページ全体の再現とは別に検証する。

## 許可する範囲

`BG2 /Default`と`UCR2 /Default`、booleanの`SA`、PDF十進数で0以上1以下の`SM`を扱う。名前のescapeと間接参照は所属ページのResourceで解決する。任意の黒生成・下色除去関数、未知のtransfer function、範囲外の値、重複属性、不正な型を通常設定とみなさない。未確認状態は後の通常設定だけでは解除せず、`q/Q`による復元を保持する。

[Adobe PDF Reference 1.6](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.6.pdf)のExtGState属性とsmoothnessの説明を参照した。`SA`はpathのstroke adjustment、`SM`はsmooth shadingの許容誤差に関する属性である。これらが画像sampleを変えないという実装上の扱いは、原本の画素と所有する合成PDFの検査で確認する。既定関数以外の色変換を推測で実装しない。

新たな設定の許可には、所属Resourceが完全で、DeviceGray・RGB・CMYKの既定色空間が置換されていないことを要求する。`DefaultRGB`などの別色空間、重複Resource、未知の合成やsoft maskは未確認のままにする。既存の非stroke alphaは一度だけ適用する。公開API・CLI・依存は変更しない。

## 原本の画素との比較

[労働力調査の固定原本と元の57条件](source_white_fill_and_adjacent_tables.md)を、既知資料への追試に使う。初回結果・gold・原本は変更しない。RGB画像1点とIndexed RGB画像5点は、元の圧縮sampleと元のpaletteから得たRGBに全画素が一致した。CCITT stencil2点は独立したdecoderの原本bitから求めたalphaと一致し、描画命令で確認した黒の塗りを保持した。同じPNG8点がDOCXへバイト一致で格納された。

試作では8件の`ImageReplaced`が解消し、本文は画像の埋め込みIDだけを置換すると親のXMLと一致した。他のZIP部品も一致し、非空白1,164文字・4表・4ページを保持した。変化した2・3ページを目視し、1・4ページは目視済み親とRGB一致した。

ただし、ロゴの位置ずれと切れ、plotの重なり、graphを誤認した大きな表、原本2→Word4ページは残る。画素の復元だけを理由にasset・graph・ページのgoldを合格へ変更しない。直前の背景の追試34合格・23未達を、画像の復元によって引き上げたとは主張しない。

## CPIと検証の限界

[CPIの固定原本107条件](../corpus/statistics-japan-cpi-prospective-gold.json)でも通常変換を追試した。native試作の変化は1・16ページに限られ、残り14ページは親とRGB一致した。画像ID以外の本文XML・非画像部品、非空白22,665文字・15表を保持した。1ページ目のheader画像は戻るが、16ページ目のロゴは下端で切れる。原本12→Word16ページやgraphの問題は残る。107条件には単独のロゴ合格条件がなく、全体の外観を合格へ変更しない。ブラウザ出力は既存の本文差があるためnativeの目視結果をそのまま流用しない。

RGB sample、stencilの色とalpha、間接属性、未知の効果からの状態復元、既定色空間の拒否、任意RGBとsmoothness・SAの48例PBTを所有する合成PDFで検査する。通常設定を許可することでstrictの画像警告が減ることはあり得るが、他の未対応要因のstrict拒否を解除しない。

この記録は試作と限定した回帰の検査であり、全資料・全runtimeの最終検証は別途固定する。`whole_document_accepted=false`と`production_ready=false`を維持する。kanpoの表・styleの[固定参照記録](source_typography_and_table_opening.md#参照)と同様に、custom licenseのコードや依存は取り込まず独立に実装した。
