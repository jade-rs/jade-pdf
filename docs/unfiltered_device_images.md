# 無圧縮Device画像

`Filter`がなく、`BitsPerComponent 8`と`ColorSpace /DeviceRGB`または`/DeviceGray`を直接宣言するImage XObjectを復元する。sampleは幅×高さ×成分数と完全一致する場合だけ受理する。sourceの`Decode`写像は既存処理を使い、Grayは写像後にRGBへ展開する。PNG化は既存のpure Rust実装を使い、依存は追加しない。

source sampleの配列を確保する前に、単一/累積展開byte数・画像保存容量・共通累積確保を予約し、画素数の上限も維持する。重複した主要属性、不完全辞書、不正なsample長、不正Decodeは成功にしない。filterの存在は未知codecをrawと推測しないための境界であり、空配列・null filterも今回の対象外とする。

mask・soft mask・filterパラメータはnull以外を今回のraw経路では受理しない。ICC・CMYK・Indexed・1 bit stencil、間接ColorSpace/BitsPerComponentはこの経路に追加しない。既存圧縮codecの対応範囲は変えない。対象外画像は従来どおり診断付きの代替画像となり、strictでは拒否する。

正しく復元した画像でも画像内の文字の完全性は未確認なので`ocr_required`を保持し、strictを無条件成功にしない。画像の色復元と文書全体のレイアウト合格は別の判定である。

公開bytes APIからRGB・反転Gray・任意RGB sampleの可逆性を検査し、不正長・未知filter・mask・不正Decode・画素上限を検査する。最初のRedとproperty seedを保持する。単一/累積展開・画像保存・共通確保のprivate guardも検証する。実PDF/native/WASMの比較は進行中であり、本番品質や全原本合格は主張しない。


## 固定した候補の検証

実装commit `d5eac81`でdefault CIの138 groupは2,443 passed / 0 failed / 32 ignored。native/WASMは同じ732 Rust fileからビルドした。既存原本・gold・親版の7,442 fileをhashで再確認した。追加の外部PDFをこの評価に含めず、既に見た自作画像を未知原本の精度へ換算しない。

nativeの84結果は69成功・15拒否で親版と同じ受理/拒否になり、66成功のDOCXはbyteまで同じだった。変化した3成功は無圧縮RGBの自作背景・前面・部分画像だけ。各DOCXのZIPを展開すると、変更されたpayloadは画像一つだけで、本文XMLとその他のpayloadは完全一致した。原本のRGB `[16,32,48]`が白 `[255,255,255]`から正しく復元され、寸法は1×1 pixelを保持した。

52入力×2 mode×2 browserの208結果はengine間でhash・error・inspectionが一致した。各engineで78成功・26拒否。変化した3件では`image_replaced`だけが消え、`ocr_required`が残り、strictは引き続き拒否した。他のinspection・拒否理由・DOCXは親版と一致し、変化した3 DOCXはnative/browserでも一致した。

親版と候補の10ページを描画し、RGBが異なる4ページを個別確認した。部分画像の色が戻り、既存の可読本文は保持した。全面画像の原本は1ページだがWord出力は親版・候補とも2ページで、本文が見えない既存のpaint-order/flow欠陥も残る。したがって画像復号部品を合格としても、これらの文書全体は未合格とする。保留中の背景配置・本文保全・source frame branchは継承していない。
