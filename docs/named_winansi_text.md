# 標準Latin書体の明示WinAnsi文字対応

ToUnicodeのない標準書体で明示されたWinAnsiを、literalではUTF-8、hexでは別の文字コードとして
読むと、em dash・Euro・アクセント付き文字・乗算や除算の記号が置換文字になる。
FontDescriptorもToUnicodeもないType1の標準Latin12書体に限り、明示WinAnsiのglyph名を
既存の固定表とAdobe Glyph ListでUnicodeへ結び付ける。

[ISO 32000-1の9.10.2とAnnex D](https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf)
に従うglyph名の対応であり、OSのCP1252変換による代用ではない。
既存のReportLab由来のWinAnsi名表をType1輪郭処理と共有し、表や依存を重複追加しない。
元のToUnicodeが存在する場合は、空・null・未対応の形式でもこの補完を適用しない。
独自font名、Symbol、TrueType、別のEncoding、辞書形式のBaseEncoding、FontDescriptorがある場合も
この変更の対象外とする。公開API・CLIは変えない。

## 処理量とメモリ

固定表の展開は原本ToUnicodeを解析した件数や入力バイト数ではない。
key/valueの保存はmetadata予算、lookup recordとcode spaceの保存はToUnicode保存容量へ、
確保前に予約する。原本のCMapや埋め込みprogramの既存の処理量制限は維持する。
共有表のキャッシュは元のFont宣言を確認してから再利用する。

最初の実装では、未使用の標準書体の表まで原本ToUnicodeの処理枠を消費し、
既存の2件・256件の境界テストが失敗した。キャッシュ位置だけの変更では解消しなかった。
固定表の保存容量と原本の解析処理を区別し、既存のテスト期待値を変えずに成功を確認した。
失敗版のソース、ログ、最初の固定変換結果は保存しており、合格版と混同しない。

## 検証結果

公開byte parserのRedから、literal・hex・Tj・TJ・quoteの6経路で同じUnicodeを復元すること、
原本ToUnicodeの優先、Encodingを省略した既存経路、未定義slotの診断を確認した。
既知値を連結する24ケースのPBTと、保存容量不足・適用条件の負例を含む。
defaultは2435 passed / 0 failed / 32 ignored、fmt・Clippyは成功している。

native64結果は59成功・5期待済み拒否で、成功したDOCXはすべて変更前とバイト一致する。
新しい未定義slotのstrict入力だけが、警告なしの成功から明示的な拒否へ変わる。
nativeでは既にSDKが通常のWinAnsi記号を復元していたため、ここを新しい文字精度の改善と数えない。

ChromiumとWebKitは各80結果で、66成功・14期待済み拒否となる。
既存36ケースのinspection・DOCX・拒否はすべて変更前と一致する。
所有する記号対照では、`A � � � � � Z`が`A — € é × ÷ Z`となり、両モードともnativeと
バイト単位で一致する。未定義slotでは通常変換に置換文字と診断を残し、strictで拒否する。
変更したDOCXは本文の期待した文字だけが異なり、書式・余白・ID・他の部品は同一である。
両ブラウザの全ハッシュ・拒否結果も一致する。

候補6選択を含む異なるDOCX7ページと所有原本2ページを個別に目視した。
記号は読めるようになるが、元の字形・字間・基線位置の完全一致を主張しない。
画像表示時の縮小もあるため、すべてを原寸で見た確認ではない。

## 未解決部分

nativeの未定義slotは、通常変換では警告を出すが置換文字自体はSDKの表示経路で落ち、
`A�Z`という保存済み期待値に対して`AZ`となる。
独自fontの対照もnativeではem dash、byte経路では置換文字となり、既存の差が残る。
この3件のnative文字列不一致を評価から除外せず、すべての文字期待値の成功とは数えない。
独自fontの厳密な字形や、未定義glyphの見た目を正解とした判定でもない。

最初のToUnicode対照に使った一行形式のCMapは有効な構文だが、既存のparserでは読めなかった。
通常の改行を持つ別の対照で、変更前の失敗とToUnicode優先を確認し直した。
一行形式の対応を直した変更ではなく、読めなかった原本宣言をWinAnsiで上書きもしない。

この検証は既知回帰資料と所有対照であり、未見の外部PDFの精度評価ではない。
原本・font・生成DOCXはprivate QAのまま、追跡PDF fixtureは3件を維持する。
一般PDFの表・数式・図・和文書式やMicrosoft Word描画の品質課題は残っている。
詳細は[検証記録](../corpus/validation-named-winansi-text.json)を参照。
