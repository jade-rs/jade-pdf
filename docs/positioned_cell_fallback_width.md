# 原本位置付きセルの代替字幅

[官報の公開ガイドの追試](kanpo_public_guide_holdout.md)では、原本の1,622文字がWord本文にあっても、描画すると23文字がセル内で切れた。MS-PGothicを代替fontへ置き換える既存の92%字幅補正が、通常の表runには適用される一方、原本の一行・複数行・収容できないセルのrunには渡っていなかった。

## 適用する範囲

原本と本文を照合した位置付きセルの三つの配置経路で、既存のMS-PGothic用の代替字幅補正を再利用する。fontを埋め込んだrunと、glyph advanceから明示の幅・字間を求めたrunは上書きしない。既存のfont size、weight、色、下線、基線、本文・段落数・セル所有を保持する。

本文とheader/footerのrun構築は変えない。段落の寄せ・相対原点の処理だけを共有し、セルで構築したrunを共通処理で作り直さない。新しいfont aliasや補正率、公開field・CLI・依存は追加していない。代替fontの近似であり、原本fontの全glyph幅の正確な一致を主張しない。

## 固定した検証

親`7348a4902ec35da19b4c3d47f2ffa6784f1a2aac`、実装`260335a1d40c18fe2f42148755e2b03acde7ab29`。Rust1.93.1のCI defaultは2,729成功・0失敗・42無効化、170テスト群。808個のRust sourceが検証・native/WASM buildを通して一致した。fmt、fixture hygiene2件、WASM distribution10件も通過した。

公開Word回帰5件は一行・複数行・収容できない配置、font alias、別family、native file、表外本文の対照を含む。32例のPBTでセルの平行移動と一様拡縮を変え、補正がセルだけへ適用されることを確認した。private回帰2件は埋め込みfontと明示glyph幅・字間の上書きを防ぐ。

最初の公開回帰は、WordのAlpha runに92%指定がないため失敗した。実際のRedで実行した808 sourceは固定コミットからbyte一致で再構成できる。Red用wrapperの期待exitを0にした誤りも保存し、Rustの実際のexit101・1件失敗と、記録後のwrapperのAssertionErrorを区別する。Redの処理は再実行していない。

native228結果（198成功・30拒否）とChromium/WebKit各236結果・118入力（200成功・36拒否）、計700結果の成否がすべて親と一致した。695 payloadはRAW一致する。変化は印紙税表・官報のnative通常と、労働力調査の三runtimeの通常出力、計5件だけである。両browserの236 payloadと118 inspectionは一致し、各inspectionは親とも一致した。これは未知PDFの正解率ではない。

変更した3文書の7ページを独立描画して、すべて個別に目視した。最終native出力は目視済み試作へRAW一致し、労働力調査の三runtimeも同じbyte列である。Wordの本文XMLは比較のために字幅指定だけを除くと親と一致し、他のZIP部品はそのままRAW一致する。実際のpayloadに正規化は加えない。

## 原本の評価と残る問題

官報は原本ページごとの1,121文字・501文字が、それぞれ編集可能なWord本文と描画に揃った。2ページを保ち、表内の文字切れは目視でも解消した。最初に固定した52条件の確認済み合格は32→38→40となる。今回は文字切れ・重なりのない表示と、全原文の元ページ保持の2条件だけを加えた。9未達と3未確認は残す。

四つの斜線と曲線の括弧、余分な節下線、原本font metrics・footer font/size/anchorは未達。タイトル強調と厳密な見出しanchorは未確認。二ページ目の本文notesの改行も原本と異なる。暗号化された元の官報PDFはbrowserでは引き続き拒否し、native strictも曲線の損失を拒否する。

国税庁の印紙税額一覧は、固定した原本の選択30セルについて番号・見出し・税額の対応を再確認した。元の比較規則どおり、見出しは空白だけを除き、番号と金額は数値の意味を照合して元の全角文字列を保存する。新しいNFKC同一視や原本glyph scalarの一致を主張しない。印刷ページ番号35/36と文書全体の外観は未達である。別資料の賞与税率表の30条件は、RAW一致による継承であり、この印紙税表とは区別する。

労働力調査の原本57条件は36合格・21未達/未確認を維持する。2原本ページがWord3ページになる問題と、vector graphの欠落は残る。画像のframeは目視済み親出力と一致し、logoの原本座標誤差も約0.031ptを維持する。CPIは各26/107をRAW一致で継承し、他資料の未達を合格へ変更しない。

83個の原本・gold・初回・保留manifest、延べ72,765ファイルを再照合した。原本や元のgold、保留結果を変えず、外部PDF・font・logoをtracked assetへ追加していない。kanpo-parser/kanpo-docxの原本と書式の対応を重視する考え方は[固定した参照](source_typography_and_table_opening.md#参照)を使い、custom licenseのコード・依存は取り込んでいない。

検証のhashと範囲は[`corpus/validation-positioned-cell-fallback-width.json`](../corpus/validation-positioned-cell-fallback-width.json)へ記録する。`whole_document_accepted=false`、`production_ready=false`を維持する。
