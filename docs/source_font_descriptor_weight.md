# FontDescriptorに明記された文字の太さ

名前が`Regular`でもFontDescriptorが700を宣言するPDFでは、名前の推測だけでは原本の太字を失う。bytes/WASM解析は、選択したfont resourceのdescriptor直下の`FontWeight`を読み、`ParsedTextElement::font_weight: Option<u16>`として保持する。simple fontとType0の単一descendant、直接値と数値への間接参照を扱う。幅情報がないfontでも独立に保持する。

採用する値は有限の整数1..=1000だけとする。属性の欠落、型違い、未解決参照、重複した属性、複数descendant、入れ子にある同名属性は未確認の`None`とする。名前や`StemV`から数値を作らない。走査は既存の停止・処理時間予算とresource resolverを共有し、依存を追加しない。同じ名前でも太さの異なるspanは連結しない。

DOCXの本文runでは、確認した埋め込みfaceの属性を最優先し、続いて有効なdescriptor値の700以上を太字、それ未満を通常体へ対応させる。太さを確認できない場合は既存の既知書体・名前による扱いを保つ。family名が欠けても明示値は独立に反映する。Wordの二値の太字へ対応させるため、300と400などの連続的なweight差を再現する機能ではない。

Rustで`ParsedTextElement`を直接構築する呼び出し側は、未確認なら`font_weight: None`を追加する必要がある。native SDKから抽出するだけのspanも`None`とする。固定したPDFium 7961の`FPDFText_GetFontWeight`は`StemV`からの推定を返すため、この属性の根拠に使用しない。native側でsourceに基づくspanを再利用する場合のコピーは値を保持するが、SDKしか読めないfontと元resourceの対応を名前だけで推測しない。

この変更は、罫線のない表の粗いfallbackにセルごとのstyleを追加するものではない。PDFの全太字、回転した見出し、段組み、改ページを含む文書全体の再現性は別途確認が必要である。

最初のRedでは解析3件、DOCX2件、native用コピー1件が失敗した。期待する`Weight`を変えずに、一行のCID CMapにある独立した復号不具合を修正した。font名がない場合の太字欠落もRedで確認してから修正した。固定した元bytes、seed、ログを保持し、全体テストとnative/WASMのファイル比較を続けている。本番品質の達成はまだ主張しない。

数値範囲は[PDF 2.0のFontDescriptor正誤表](https://pdf-issues.pdfa.org/32000-2-2020/clause09.html)を参照した。PDFiumの推定経路は[固定7961のCPDF_Font](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/7961/core/fpdfapi/font/cpdf_font.cpp)と[FPDFText_GetFontWeight](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/7961/fpdfsdk/fpdf_text.cpp)で確認した。
