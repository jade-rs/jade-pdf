# 描画による文字の強調

FontDescriptorがweight400を宣言していても、PDFは同色の塗りと細い輪郭線を重ねて文字を強調できる。通常のweight値を書き換えず、確認した描画の線幅を別の属性として渡す。

## Rust API

`ParsedTextElement::fill_stroke_width: Option<f32>`は、非Type3のfont種別、文字描画モード2、同じDevice色のfillとstroke、所属ページの通常合成と不透明な描画を確認した実効線幅である。`font_size`と同じ座標単位を使う。未確認の要素、nativeだけの未照合span、OCRや生成した空白では`None`にする。

Rustで構造体を直接構築する呼び出し側は、未確認なら`fill_stroke_width: None`を追加する。`font_weight`は実際のdescriptor値を保持するため、描画の強調があっても400のままである。nativeでsourceの位置・本文を再利用する照合経路では、線幅もコピーする。異なる描画の要素を結合しない。

## 確認条件と適用範囲

DeviceGray・RGB・CMYKの成分を有限値として検査し、PDFの成分範囲へclipする。同じ色空間の同じ成分だけを同色とする。未知の色空間・`sc/scn`は根拠にしない。`q/Q`は色と線幅を復元し、非strokeとstrokeの状態を独立して扱う。

一様で直交する有限のCTMだけから実効線幅を求める。文字のemは`Tf`と文字行列から決まるため、`Tf 1`と拡大した`Tm`を使う原本でもemとの比率を保つ。フォント未選択、種別未確認、Type3、非一様なCTM、非正・未確認の線幅から強調を推測しない。

ページのResourcesは検証済みの展開metadataから読む。Device色の既定置換がなく、ページ内の全ExtGStateが既知で、fillとstrokeがともに不透明、通常合成、soft maskなしと確認できる場合に限る。転写関数や未知の効果は従来の表示へ残す。配列や文字列の中の偽命令、Contents境界、`Tr`と`w`の不正なoperandも検査する。位置・状態の保存には既存のmetadata予算を使う。Form内の局所Resourcesはこの経路では確認しないため、Form内の線幅は`None`を保つ。

Wordでは、線幅がemの1～4%のときだけ、確認した描画の強調を太字へ対応させる。descriptorや埋め込みfaceの通常体とは別の根拠として扱い、同じ行や表の隣の通常runへ広げない。線幅そのものをWordの太字で再現したとはしない。異なるfont faceの字形・幅や、強調以外の配置は別途確認する必要がある。strokeだけの輪郭文字、透明な線、別色、hairline、太い線、clipを兼ねる描画の厳密な再現は、この機能の対象外である。

## 表現の確認

Wordの[`w14:textOutline`](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.office2010.word.textoutlineeffect?view=openxml-3.0.1)を付けた試作と互換用の代替指定は、現在のLibreOffice描画検証では見た目を変えなかった。その試作を承認済みの出力や本番品質の根拠にしない。通常の太字指定は描画へ反映されることを、同じ原本の文字、通常体の隣接run、編集可能な表で検査する。

労働力調査とCPIの原本・初回結果・goldは、それぞれの[表の確認記録](source_white_fill_and_adjacent_tables.md)と[数値欄の確認記録](source_numeric_tj_positions.md)に固定したものを使う。既知資料への追試であり、未見文書の精度率とは扱わない。graph、色、font、logo、物理ページ対応など、元の未達をこの強調だけで合格へ変更しない。

kanpo-parser/kanpo-docxの通常体と強調を区別する考え方は[固定した参照記録](source_typography_and_table_opening.md#参照)を参照。custom licenseのコード・依存は取り込まず、既存の状態追跡、IR、Word builderへ独立に実装する。
