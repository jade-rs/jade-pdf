# native clipの文字索引

部分clipを持つtext objectを調べる際、従来はSDKの`text.chars(page_text)`を
objectごとに呼び出していた。このAPIはページ全体のindex配列を生成してから、
そのobjectに所属する文字を選ぶ。短いobjectが多いページでも全文字数に比例する
配列の確保と予約が毎回発生し、metadataの累積予算を使い切ることがあった。

現在は最初の部分clipでページ文字の共有配列と所属索引を一度だけ作る。
公開された矩形・変換行列が全text object列で一意な場合に限り、SDKから取得した
所有objectの幾何を使って元の文字indexを検索する。全候補にはForm内のleafと
同じ幾何を持つ別objectも含める。重複・不明な幾何・不完全なobject列では、
従来の厳密なSDK所属検索と予約へ戻す。字面や矩形の重なりだけで所属を決めない。

追加配列は既存のmetadata予算へ確保前に予約し、Formの深さ・停止状態も維持する。
既定の128MiBを変更していない。通常の部分clipなしページでは索引を追加しない。
曖昧なobjectを含むページには従来経路の作業量が残り、全入力の線形時間を保証しない。

所有する160個の異なる部分clip付きobjectは、旧経路で256KiBの予算を超えた。
共有索引では同じ予算で同一maskを返す。32例のproperty test、同一幾何・異なるclip、
共有Form・回転のテストでは、最適化と独立したSDKの厳密な所属APIとmaskを比較する。

事前に全ページ・権利・独立goldを確認した国税庁の日本語表（604,798 bytes、2ページ）は、
旧版でmetadata予約134,230,384 bytesが既定134,217,728 bytesを超えて解析に失敗した。
修正後は同じ既定予算で2ページと非空の本文を解析できる。原本はコピー許可のある
暗号化PDFで、nativeの既存処理を使う。pure/WASMの暗号化入力拒否は変えていない。

同じnative正規化を通した原本の両ページについても、SDKの全5,195文字の各indexで
旧所属検索と除外maskが一致した。事前goldの30セルは番号・種類・税額の論理対応を
保持し、DOCXは4列の表を2つ、描画後も2ページになった。これは文字コードの完全一致や
資料全体の合格を意味しない。第1ページの大きいセルは書式のない本文へfallbackして
文字が切れ、狭い番号セルでは二桁の番号が折り返される。見出し画像のdecode失敗と
曲線pathの未対応も残り、strictは4件の品質問題で出力を拒否する。

mainの画像・罫線修正を含む候補では、既存14入力のnative DOCXは変更前とbyte単位で
一致した。BatchNormの曖昧な部分clip拒否も維持した。WASM配布物10ファイルも変更前と
一致したため、既存ブラウザ検証の結果を再利用しており、新規ブラウザ実行は主張しない。
default featureのClippyと全テストは2,390件成功・32件ignore・失敗0件だった。
検証範囲は[記録](../corpus/validation-native-clip-workspace.json)を参照。

出典：国税庁「[印紙税額の一覧表](https://www.nta.go.jp/publication/pamph/inshi/tebiki/pdf/08.pdf)」
（令和8年6月現在）。本検証用goldは原本の一部を選択・加工して独立に作成した。
[原本登録](../corpus/manifest-japanese-table-holdout.json)と
[事前gold](../corpus/nta-stamp-tax-table-2026-06-prospective-gold.json)を参照。
