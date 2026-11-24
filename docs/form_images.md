# Form 内画像の抽出

Form XObject の局所 `Resources` を使って、実際に `Do` で呼ばれた Image XObject を抽出する。
ページ、親 Form、子 Form、画像の行列を合成し、同じ画像の再描画を別の配置として保持する。
未使用のページ別名があっても、Form 内で描いた共有画像を除外しない。

ページの CropBox、Form の BBox、対応済みの凸 path clip を交差する。
完全に見えない画像は除外し、部分的な切り抜きでは外側を透明にした PNG を生成する。
PNG の保存解像度は元画像の画素密度と 96dpi の表示寸法の大きい方を使う。
表示寸法と保存画素数は分離し、有限な小さい行列を特異行列と誤認しない。
切り抜き頂点、画像配置、PNG と画素列は既存の累積容量・画素数上限へ計上する。
停止や容量不足を、部分的に成功した文書として返さない。

通常 Form は呼出側の対応済みの色・alpha を継承し、`q/Q` と子 Form の変更を外へ漏らさない。
グループ単位の透明度、分離、knockout、未対応の色変換などは個別画像の alpha で代用せず、既存の画像代替診断へ渡す。
不透明な通常 RGB グループの画像成分を回収できても、グループ全体を描画できたとは扱わない。

Form 画像は本文の inline 順序へ置き換えず、位置を持つ画像として出力する。
正立ページの floating 画像は、PDF の下端原点を `page.height - image.bounds.y1` で上端原点へ変換する。
横・縦とも Word のページを基準にし、section の余白を二重に足さない。
ページ寸法不明、ページ自体の回転、画像回転の全組合せは今回の描画受け入れ範囲に含めない。

## ASCII85 の本文

ページと Form の本文で、直接の `[/ASCII85Decode /FlateDecode]` を宣言順に復号する。
`DecodeParms` は省略、`null`、`[null null]` に対応する。
両段階の復号量を共通の上限へ累積し、破損や未対応の ASCII85 の順序・parameter は明示的なエラーにする。
他の一般的な filter chain、間接 filter/parameter、予測変換を網羅するものではない。

## 検証と残る問題

所有するメモリ内 PDF の Red → Green → Refactor で、局所参照、再描画、入れ子、同名 resource、alpha の復元、BBox と凸 clip、微小行列、保存画素密度、資源上限を検証している。
ASCII85 の任意 byte 復元・二段の累積上限と、小数を含むページごとの floating 原点には property test がある。

ReportLab で作成した所有する 1 ページ PDF は、修正前には本文の filter chain のため空文書になり、strict でも欠落を検出しなかった。
入力を変更せず、修正後は 3 回の画像描画と切り抜きを保持する。
画像位置は復元できるが、本文ラベルの縦位置や切り抜き辺の rasterization は完全一致ではない。

[EfficientNet の原本](https://proceedings.mlr.press/v97/tan19a.html)は 10 ページを事前に確認した。
Figure 7 の 12 枚について、元画像と原本の矩形 clip を独立に解析し、切り抜き後の RGB 画素を native と Chromium/WebKit の実際の DOCX で照合した。
画像 anchor の左端・上端は独立参照から 0.01pt 以内だが、表示寸法には px 丸めによる最大約 0.355pt の差が残る。
最初の独立参照は clip 適用前の bounds だったため、原本の clip を解析した参照を観察後に追加している。
元の参照と不一致の記録は残し、未使用 holdout の事前合格とは主張しない。

12 枚は描画されるようになったが、既存の大きすぎる繰り返し見出しとの重なりがあり、原本 10 ページが出力 20 ページになる問題は残る。
段組み、表、数式、ベクター図形、フォント置換を含む文書全体の精度は不合格のままである。
Table 1 の事前に保存した 9 行 × 5 列、45 数値の参照は変更していない。

[BERT の原本](https://aclanthology.org/N19-1423/)では、16 個の共有 bitmap asset による 38 回の描画を回収した。
これは図の bitmap 成分であり、欠けている vector の塗りや本文・図ラベルの配置を復元したものではない。
原本 16 ページが 32 ページになる問題は残る。
両論文の既存の本文テキスト要素は維持され、公開 fixture 3 件の native DOCX byte 列は変わっていない。

`UnsupportedXObject(Form)` と `OcrRequired` は引き続き出力し、strict で拒否する。
Form、透明合成、非凸・曲線 clip、全描画順、すべての vector artwork に対応したとは扱わない。
確認に用いた viewer は LibreOffice であり、Microsoft Word での描画は未検証である。
配布用のフォント許可判定、元 PDF、選択した表の参照、元のフォントプログラムは変更していない。
実文書の欠落は残っており、プロダクション準備完了とは扱わない。

## DeviceCMYK JPEG の色と制限

8 bit の `DeviceCMYK` + `DCTDecode` は CMYK/YCCK の四成分を保持して復号し、PDF の `Decode`、検証済み四成分の soft-mask `Matte`、既存の RGB 近似の順に処理する。
Adobe APP14 の transform と画像寸法・成分数を検証し、不正な Decode/Matte を推測で補わない。画像 pixel 上限と strict の品質拒否も維持する。
名前の escape、singleton 色空間、間接参照、明示 identity Decode は、同じ画像画素を保持する。

RGB 化は既存の unmanaged DeviceCMYK 近似である。[PDF 32000-1 の 10.3.5](https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf)にある基本的な変換式に従うが、印刷機の色や CMS による参照描画を完全に再現するものではない。
NASA Webb factsheet の両ページでは背景の反転が解消した一方、暗部の細部はつぶれる。原本 2 ページが出力 6 ページになり、画像と本文の分離、白い本文の不可視化、誤った表推定、vector の欠落も残る。
両背景画像以外の native DOCX part、Chromium/WebKit の本文 XML・品質警告・strict 拒否は変更前と一致し、NIST wallet の出力も変更前と同一である。
この検証は色極性の修正を確認するもので、factsheet 全体やカラーマネジメントの合格を意味しない。[検証記録](../corpus/validation-device-cmyk-jpeg.json)を参照。
