# 原本のweightと表の先頭行

原本の埋め込みfaceから読めるweightはfont名より優先する。明示された通常体は
`w:b`と`w:bCs`をfalseにして、先に付いた見出しの太字も解除する。
埋め込みfaceを確認できない場合は、対応済みのRoman互換face、または既知のstyle接尾辞を使う。
`Regular`、`Light`、`Medium`などの通常体と、`Bold`などの太字を区別し、
family名に含まれる`black`や`bold`、Gothicというfamily、不明なW番号だけでは太字にしない。
未知のstyleは既存のrun指定を保持する。埋め込みfontの権利とfont代替の既存の条件は変えない。

二行以上の編集可能な表は、先頭行の各セルの末尾にある直接の段落へ`keepNext`を指定する。
表の先頭行と次の行がページをまたいで孤立することを避けるためのWordへの指示で、
繰り返し見出し、行全体の分割禁止、固定改ページは追加しない。
一行だけの表や、末尾が入れ子の表であるセルは指定せず、子表の内部へも降りない。
原本の列幅・結合・罫線・文字を推測で変更しない。

この指定のMicrosoft Wordでの表示、表全体の検出精度、原本と同じページ数は別に検証が必要である。
公開API・CLI・依存の追加はない。

## 参照

ユーザー指定の[`bokuweb/lawrenceanum`](https://github.com/bokuweb/lawrenceanum/tree/8322ad7895d8e15f7002c80e3f67c993edce99fd)
の`kanpo-parser`と`kanpo-docx`を固定コミットで調査した。
[元fontのfamilyとweightを分ける設計](https://github.com/bokuweb/lawrenceanum/blob/8322ad7895d8e15f7002c80e3f67c993edce99fd/crates/kanpo-parser/src/media_font_styles.rs)、
[文字由来の通常体が太字を解除する扱い](https://github.com/bokuweb/lawrenceanum/blob/8322ad7895d8e15f7002c80e3f67c993edce99fd/crates/kanpo-docx/src/annotations.rs)、
[通常表の先頭行を後続へつなぐ扱い](https://github.com/bokuweb/lawrenceanum/blob/8322ad7895d8e15f7002c80e3f67c993edce99fd/crates/kanpo-docx/src/docx.rs)
を参考に、jadeの既存IR・DOCX構造へ独立に実装した。
参照リポジトリは[独自ライセンス](https://github.com/bokuweb/lawrenceanum/blob/8322ad7895d8e15f7002c80e3f67c993edce99fd/LICENSE)であり、
ソースコードのコピーや再ライセンス、官報PDFの取り込み、依存の追加は行っていない。
官報専用の記事先頭の太字化は、一般PDFの内容からは判断せず適用しない。
