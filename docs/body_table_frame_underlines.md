# 表外枠を本文の下線に使わない

官報の公開ガイドでは、表のすぐ上にある三つの節見出しへ、原本にはない下線が付いた。本文の下線推定が表の上辺も候補として使っていたためである。国税庁の印紙税額一覧と消費者物価指数でも、表の外枠が近い見出しや表題に同じ問題があった。

## 原本の四辺を確認する

原本文字を照合したセルを含む位置付き表について、同じページに有限な正の線幅を持つ実際の四辺が揃っている場合だけ、本文の下線候補から全幅の上辺・下辺を外す。独立した下線、短い線、不完全な枠は従来どおり保持する。四辺の端点は既存の整数gridとの1pt許容範囲で確認し、対向辺の許容範囲が重なる小さな枠は推測しない。

元のgraphicsは変更せず、本文の書式推定用のコピーだけを絞る。表の推定・描画・セル内の書式や、公開field、CLI、依存は変えない。元の座標がNaNや無限大でないことを、min/maxによる正規化より先に確認する。

## 固定した検証

親`7f4696f9c7e2bb2b21c120048175d8ceca1e8a74`、実装`ee44f66b6b585de081f9f67bdd1caea8f37b1894`。Rust1.93.1のCI defaultは2,739成功・0失敗・42無効化、171テスト群。810個のRust sourceは全体検証・native/WASM buildを通して一致した。fmt、fixture hygiene2件、WASM distribution10件も通過した。

公開Word回帰5件は、不要な見出し下線、独立した下線、右辺のない枠、native変換と入力不変、32例の移動・一様拡縮PBTを含む。private回帰5件は四辺・同一ページ、部分線・端点、無効な線幅・小さな枠、全座標のNaN/正負無限大、32例のframe geometry/page PBTを含む。

実際に失敗した公開Redと非有限座標のprivate Redの実行sourceは、固定コミットからそれぞれ809/810ファイルをbyte一致で再構成できる。最初の実装では任意のmerged-cell modelを通常の完全gridにも要求してしまい、Greenは失敗した。そのログと810 sourceを保存し、合格へ数えない。最終実装は原本セル所有と実際の四辺を根拠にする。Redや失敗Greenを再実行して証拠を置き換えていない。

native228結果（198成功・30拒否）、Chromium/WebKit各236結果・118入力（200成功・36拒否）、計700結果の成否は親と一致した。695 payloadはRAW一致する。変化したのは印紙税表・官報・CPIのnative通常と、両browserのCPI通常の5件だけである。両browserは236 payload・118 inspectionで一致し、inspectionは親とも一致した。この値は未知PDFの正解率ではない。

変更したnative3文書の20ページと、別のbyte列を持つbrowser CPIの16ページを描画した。native7ページ、browser3ページを個別に目視し、残るCPI各13ページは目視済み親とRGB一致した。最終nativeは目視済み試作とRAW一致する。各物理ページの描画文字multisetも親と一致する。変更XMLは比較のために下線要素だけを外すと親と一致し、他のZIP部品はRAW一致する。実際のpayloadは正規化しない。

## 原本の評価と残る問題

官報の余分な三つの節下線を除去し、原本ページごとの1,121/501文字が編集と描画に揃った状態を保持した。最初に固定した52条件は40確認済み合格・9未達・3未確認のままにする。下線除去はそのgoldの独立条件ではないため、合格数を増やさない。四つの斜線、曲線の括弧、原本font metrics、footerと厳密な見出しanchorは引き続き未達または未確認である。

印紙税額一覧は元の比較規則による選択30セルの番号・見出し・税額の対応を保持する。別資料の賞与税率表の30条件はRAW一致による継承と区別する。印紙税表の原本ページ番号35/36は依然表示されない。

CPIは三runtimeとも26/107を保持する。編集可能な22,665文字を全部表示できたとはせず、描画は22,415文字のままである。グラフ線の欠落、セル内の文字切れ、原本12ページからWord16ページへの増加、browser最終ページのlogoと本文の重なりは残る。労働力調査はRAW一致で36/57を継承し、原本2ページからWord3ページへの増加とvector graph欠落は未達である。元の暗号化された官報・印紙税PDFはbrowserで引き続き拒否し、native strictも既知の品質損失を拒否する。

84個の原本・gold・初回・保留manifest、延べ74,952ファイルの内容を再照合した。外部PDF・font・logoはtracked assetへ加えない。kanpo-parser/kanpo-docxからは、原本にある構造と書式を対応させて確かめる考え方を[固定した参照](source_typography_and_table_opening.md#参照)として使い、custom licenseのコード・依存は取り込んでいない。

hashと範囲は[`corpus/validation-body-table-frame-underlines.json`](../corpus/validation-body-table-frame-underlines.json)へ記録する。`whole_document_accepted=false`、`production_ready=false`を維持する。
