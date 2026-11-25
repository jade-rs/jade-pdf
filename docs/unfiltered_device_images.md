# 無圧縮Device画像

`Filter`がなく、`BitsPerComponent 8`と`ColorSpace /DeviceRGB`または`/DeviceGray`を直接宣言するImage XObjectを復元する。sampleは幅×高さ×成分数と完全一致する場合だけ受理する。sourceの`Decode`写像は既存処理を使い、Grayは写像後にRGBへ展開する。PNG化は既存のpure Rust実装を使い、依存は追加しない。

source sampleの配列を確保する前に、単一/累積展開byte数・画像保存容量・共通累積確保を予約し、画素数の上限も維持する。重複した主要属性、不完全辞書、不正なsample長、不正Decodeは成功にしない。filterの存在は未知codecをrawと推測しないための境界であり、空配列・null filterも今回の対象外とする。

mask・soft mask・filterパラメータはnull以外を今回のraw経路では受理しない。ICC・CMYK・Indexed・1 bit stencil、間接ColorSpace/BitsPerComponentはこの経路に追加しない。既存圧縮codecの対応範囲は変えない。対象外画像は従来どおり診断付きの代替画像となり、strictでは拒否する。

正しく復元した画像でも画像内の文字の完全性は未確認なので`ocr_required`を保持し、strictを無条件成功にしない。画像の色復元と文書全体のレイアウト合格は別の判定である。

公開bytes APIからRGB・反転Gray・任意RGB sampleの可逆性を検査し、不正長・未知filter・mask・不正Decode・画素上限を検査する。最初のRedとproperty seedを保持する。単一/累積展開・画像保存・共通確保のprivate guardも検証する。実PDF/native/WASMの比較は進行中であり、本番品質や全原本合格は主張しない。
