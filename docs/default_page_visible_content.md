# 通常変換で表示枠の外の文字・画像を再配置しない

PDFは表示領域の外にも文字や画像を持てる。通常のWord変換がその要素を段落や画像として再配置すると、PDFには見えない内容が現れ、余分なページや折返しを作る。[PDF Reference 1.5](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.5_v6.pdf)はCropBoxの既定をMediaBoxとし、表示・印刷する内容を表示領域へ切り取る。

## 対象と保持する情報

通常の最小構成file/bytes変換で、所有する変換用IRの文字と画像だけを判定する。一意なページ番号、有限で正の寸法、直角回転から表示寸法を確認し、有限で正の面積を持つ要素が完全に表示枠の外にある場合に除外する。90度・270度では幅と高さを交換する。未知・重複ページ、不正寸法、非直角回転、未確認のbounds、一部だけ交差する要素は従来の経路へ残す。公開parserの抽出結果と入力PDFは変えない。

全要素が表示外でも、raw text previewから本文を復活させない。罫線は個別に除外しない。最初の実装は表示外の罫線も除外し、部分表示された表の行高を壊したため、採用していない。元のPBT、失敗ログ、最小入力top32/bottom161と回帰seedを保持し、期待値を変えずに文字・画像の範囲へ絞った。

回転ページの右寄せ単一runでは、表示内の縦長source boundsと行の左右端・文字列が一致するとき、既存の横書きfallbackへ元のadvanceに対応する幅を確保する。表示外要素を除去しただけの試作で270度のVisibleが一文字ずつ折れた問題を、独立描画で確認して修正した。これは文字の完全な回転・原本の座標・font metricsの再現を保証しない。OCR APIの新しい表示外フィルタやモデル精度改善はこの変更の対象にしていない。公開field・CLI・依存の追加はない。

## 固定した検証

親は`d0c848e75c1102ec122027a1176eaf7b95196d70`、実装は`fadd9438d35de95b206ea894bbc33df16e8b7756`。Rust1.93.1のCI defaultは2,691成功・0失敗・42無効化、163テスト群、799個のRust sourceが実行前後で一致する。fmt、fixture hygiene2件、WASM distribution10件も通過した。公開回帰7件には交差・回転の32例PBTを含み、private回帰3件で曖昧なページ・boundsと罫線の維持を確認した。3回の実際のRedは固定コミットから実行した全Rust sourceをbyte一致で再構成できる。

既存native194件とChromium/WebKit各202件、計598件の成否とpayloadはすべて親にbyte一致し、各101inspectionも一致した。これとは別に、4回転それぞれの表示内対照・表示外の所有PDF8件を追加した。各runtimeの通常8件、strict表示外4件が成功し、strict表示内4件はOCR reviewで拒否する。新しい48件は宣言した成否に一致した。これは既知の所有回帰であり、未知PDFの正解率として数えない。

新しい比較poolはnative210件（181成功・29拒否）、各ブラウザ218結果/109入力（184成功・34拒否）、計646結果。両ブラウザの218payloadと109inspectionは一致する。追加した8件の実行順序も入力へ固定する。

nativeの通常8件とstrict成功4件は目視した試作にbyte一致した。ブラウザの通常3件はnativeにbyte一致し、異なる5件は独立描画して各ページを確認した。Visibleは分断されず、Outsideは現れず、表示内だけに画像が残る。表示内のnative4対照は親とbyte/RGB一致。ブラウザ4対照のうち1件はbyte一致し、3件は同じ実行履歴で生成される画像relationship IDだけが変わった。PNGと他partは一致し、本文とrelsはそのID/Targetの対応だけを照合した。幾何・文字・styleは正規化しない。native/browserの実際の余白・色等の差も残している。

追加のupper-strip確認は4回転×画像有無×native/bytesの16呼出しが成功した。修正を要する不具合は再現せず、試験source・実行バイナリ・ログをローカル記録へ保持して、追跡sourceを固定実装とbyte一致へ戻した。この補助確認は2,691件や646結果へ加算せず、Redという最初のログ名も製品不具合の証拠にしない。

80個の原本・gold・初回・保留結果manifest、延べ62,859ファイルを照合した。既存payloadのbyte一致で、労働力調査36/57（21未達）、CPI26/107（81未達）と各runtimeの描画を継承する。新しい原本条件の合格は増えていない。労働力調査はベクターのグラフがなく、文字が次ページへ流れ、原本2ページがWord3ページになる。部分画像のclipping、表示外の図形全般、回転文字・font・改ページ等も未達のまま。

集計・hash・残件は[`corpus/validation-default-page-visible-content.json`](../corpus/validation-default-page-visible-content.json)に記録する。外部PDF・font・ロゴを追跡assetへ追加せず、`whole_document_accepted=false`、`production_ready=false`を維持する。
