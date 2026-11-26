# 非記号TrueTypeの明示WinAnsi復号

原本にTrueType、WinAnsiEncoding、FontDescriptorのNonsymbolic flag（32）が明示され、Symbolic flag（4）がなく、ToUnicode自体も存在しない場合に限り、既存のAnnex D対応表で単byte文字を復号する。節記号・通貨・Latin文字をUTF-8として処理して置換文字へ変えることを防ぐ。

原本ToUnicodeは空・null・不正参照でも上書きしない。記号font、矛盾するflag、非整数flag、MacRoman、Encoding辞書とDifferencesはこの経路に入れない。未定義slotは未知のままとして診断し、strictでは拒否する。固定対応表にも共有metadata workspaceを課し、停止時は部分表を公開しない。元のfont名、font program、書式・画像・tableの構造は変更しない。新しいruntime依存は追加していない。

根拠は[PDF32000-1 §9.6.6.4、Annex D](https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf)の非記号TrueTypeとnamed Encodingの規則であり、font名から文字を推測しない。

## 未見資料と検証

[GSA SF94、改訂11/2020](https://www.gsa.gov/reference/forms/statement-of-witness)を公式配布元から取得した。原本848,850 bytes、SHA256は0500fcb8995e1a625dfb110f8d6c8e88bcc0ceed95f6694210bbdb2e34c7b0ff。原本3ページを変換前に個別確認し、29欄すべて空欄で個人情報の記入がないこと、第三者creditがないことを確認した。[GSAの再利用方針](https://www.gsa.gov/website-information/website-policies)と標準連邦機関formとしての出典から米国内public domainと推定する。CC0とは扱わず、GSAの第三者素材に関する例外も記録した。

変換前に、原本3,641非空白文字の個数、literal label、太字・斜体、入力欄・罫線・余白、図の配置、ページ対応を含む71項目を固定した。qualified親実装による初回の未見評価はnative default 56成功・15失敗、Chromium/WebKit default各58成功・13失敗。strictは3環境とも画像/OCR診断で拒否し、原本再現の成功には数えない。この初回結果とgoldは保存したままである。

公開byte APIのRedを先に保存し、5公開テスト（24例propertyを含む）と2つのbudget/優先順位テストを通した。default全体検査は2,571成功・0失敗・42ignored、150 groups。773 Rustファイルを固定したnativeとWASMを使用した。

CLI190回（167成功・23期待拒否）、Chromium/WebKit計396回（各170成功・28期待拒否）、合計586回を確認した。既存580成果物はqualified親とRAW byte一致する。新しいSF94のnative2件とbrowser strict2件も初回出力・拒否と一致する。変更はbrowser default2件だけで、word/document.xmlの4つの置換文字が原本の§へ戻った。他のpackage partはRAW一致し、3環境のdefaultすべてで原本3,641非空白文字の個数が一致する。browser inspectionの変更もpreview内の§1文字だけである。

両engineは198成果物・99検査で一致する。過去の表・太字・2列散文などの検証は既存580成果物のRAW同一性から継承する。既存3列散文のtable非発明gold失敗6件も引き継ぎ、成功に読み替えない。限定したnative/browserの108 RAW一致・4生成段落ID差と7ページRGB照合も同じbyte成果物から継承する。新SF94のnative/browser成果物自体は異なり、一般的な決定性は主張しない。

修正版browserの出力6ページを個別確認した。nativeの6ページは初回とRAW一致するためレビューを継承する。71項目の既知追試はnative56成功・15失敗、各browser59成功・12失敗。初回の未見評価を変更したり、追試を未見精度として数えたりしない。親・原本・gold35,071ファイルも変更なく再照合した。[検証台帳](../corpus/validation-truetype-winansi.json)にhashと範囲を記録する。

## 残る問題

SF94は原本3ページから6ページへ増え、footerが余分なページへ流れる。formの罫線と29入力領域の対応、header列、nativeの分割された一部label、図の相対配置、font置換による見た目が未達である。画像6個の保持だけでは原本配置や画像内文字の完全性を承認しない。strictの画像/OCR拒否も維持する。

Censusの見出し下線・配置、NAOJの図形・日付区切り、3列散文への表の誤生成や一般の改ページも別の課題として残る。この変更は文字復号のcomponent検証であり、文書全体のproduction readinessは未達である。
