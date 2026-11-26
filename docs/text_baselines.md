# 正立した水平文字の基線

`ParsedTextElement::baseline_y` は、文字上げ（`Ts`）と文字行列・CTM・Form行列を合成した
ページ座標の文字描画原点を保持する。bytes/WASM経路では、派生Type1に加え、
対応済みの水平CID経路とsimple fontでも設定する。
通常のfontでは同じ基線の連続文字を従来どおりspanへまとめ、派生Type1の元code単位の分割は維持する。

書字方向、有限かつ正のfont size、有限な正立行列を確認する。
正立判定は合成後のページ空間で行い、正の縦横軸を持ち、回転・斜交成分がないことを要求する。
反転を二回合成して最終的に正立した場合は確認できる。
負のfont size、回転・斜交・反転、未確認の書字方向では`None`を保持する。
既存の対応済み水平CID metricsを再利用し、追加のCMap展開やfontファイル解析は行わない。

bytes/WASMでは、合成済みの確認済み基線へ有効MediaBox/CropBoxの表示原点を矩形と同じように反映する。
ページ回転後に正立を確認できない場合は`None`へ戻す。一部表示された文字の基線は表示枠へ丸めない。
回転本文の数字を保持する範囲と未達は[表示原点と基線の検証](page_viewport_text_baselines.md)を参照する。

CMapの書字方向は文字行列の向きと別に確認する。
`Identity-V`、埋め込みCMapの`WMode 1`、未解決CMapを水平書字と扱わない。
Adobeの[font API](https://opensource.adobe.com/dc-acrobat-sdk-docs/acrobatsdk/apireference/PDFEdit_Layer/PDEFont.html)にも、水平と垂直の書字モードを別の値として定義している。

同じBaseFont名のfont aliasでも、一方が既知の基線、他方が未知ならspanを結合しない。
両方が既知の場合は基線差0.0001pt以内でだけ結合する。
文字・font size・font名・色・矩形の既存の結合条件も維持する。
`baseline_y: None`を指定する呼び出し側は、従来の`bounds.y0`による行復元を使える。

これは既存の公開fieldへ設定する情報の拡張で、APIの形・CLI・依存は追加しない。
fontの埋め込み許諾、Unicodeの確認、clip、OCRの診断とstrictの拒否は維持する。
DOCX側の[添字位置と行回復](inline_baselines.md)は、確認済みの値を使って相対位置を保存する。
数式構造、縦書きの完全な組版、表や図の再構成、改ページやMicrosoft Wordの忠実度の保証は含まない。
