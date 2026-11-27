# 不透明なRGBページGroupと表の背景

ページ辞書に`/Group`があるだけで背景の証明を破棄していたため、不透明な青い表見出しも白いセルになっていた。既存の矩形・描画順・全セル被覆の証明に加え、確認できたRGBページGroupだけを通す。

`/S /Transparency`と明示的な`/CS /DeviceRGB`を必須にする。`/Type`は省略または`/Group`、`/I`と`/K`は省略または`false`に限定する。`I=true`も現段階の対象外であり、PDF仕様上無効という意味ではない。完全な辞書・一意な属性・同じ所属ページを確認し、既存の予算付きresolverで間接参照を解決する。未知のキー、重複や別綴りによる重複、未解決参照、他の色空間は推測しない。

ページGroupの確認は、不透明な通常合成、soft maskなし、Device色の既定置換なしという既存のpaint検証を置き換えない。部分的な塗りをセル全面に広げず、本文を画像化しない。公開API・CLI・依存は変えない。

判断の根拠は[PDF 32000-1:2008](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf)の11.4.7と11.6.6、Table147である。通常のページ出力の合成と、別ページへ組み込むGroup XObjectの合成を区別する。Formへの対応を追加する変更ではない。

## 新しく収集した原本と変更前の基準

[W3Cの結合セルの表](https://www.w3.org/WAI/WCAG21/working-examples/pdf-complex-table/complex-table.pdf)を収集し、1ページ全部を目視した。出典と[W3C document license](https://www.w3.org/copyright/document-license-2023/)をローカルNOTICEに残し、PDFやfontをGitに追加しない。原本SHA256は`c0bd0fc65103f2528e2fd6ece4c2529cce9dbbbdc0d333dc0d7e538550cd46c7`。

最初の製品変換より前に、原本の黒い細矩形の中心線、31論理セル、4つの縦結合と1つの横結合、291非空白文字、font・size・weight・青色を193条件として固定した。gold SHA256は`e662774f77d13eae4c9eb8fdb19fe6be6fb3328960800b03f2caecfc0843d4b6`。青い塗りの内部矩形を罫線と誤認するため、独立ツールの既定の表検出結果はgoldにしない。

最初の未見評価は保留中の別候補`1056d5c`で181合格・12未達だった。native通常/strict、Chromium/WebKit通常/strictの6出力はRAW一致した。初回の欠落、gold、原本、全出力をそのまま残す。この資料は固定772件の回帰集合へ混ぜない。

## 既知資料としての早期追試

この修正は確認済み親`293fc19`から分岐する。親の追試は184合格・9未達、修正版は191合格・2未達だった。初回181/193からの未見精度の向上率とは扱わない。追加合格は7セルの青い背景だけであり、変換後の基準変更はない。

親・候補の各1ページを個別に目視し、青い7セルの表示を確認した。DOCXの他のZIP部品はRAW一致し、本文XMLも7つの`w:shd`だけを除くとRAW一致する。描画PDFの全文字位置・font・size・用紙寸法・黒い罫線も親と一致した。通常/strictの候補DOCXもRAW一致する。

背景矩形の非strokeのedgeへ付いた既定の線幅を罫線として数えてしまった最初の測定ログも残した。追試では実際にstrokeされた黒い線だけで、変更しない原本の線幅条件を測る。

上端罫線は原本から−0.75pt、次の罫線は−0.65ptで、固定した0.5ptを超える。見出しや本文の縦位置も原本と異なる。193条件の合格だけで字形基線の完全一致やMicrosoft Wordでの表示を保証しない。全文書として`whole_document_accepted=false`、`production_ready=false`を維持する。

## 検証状況

正式な生成PDFで、既定Group・間接参照・別綴り・ページ間所有・未知の合成・半透明・soft mask・既定色置換・部分clipを確認する。PBTの失敗seedと実際のRedを保存した。DOCXのnativeファイル経路とbytes経路で、編集可能な青い見出し・太字・通常本文を確認した。

最初の全体実行は、以前の「全ページGroupを未確認とする」生成fixtureの期待値で停止した。失敗したRAWログと元のテストを残し、同じ既定RGB入力に原本どおり2セルの背景を求める期待へ更新する。未対応のGroupの否定例を別に残す。収集原本のgoldや初回結果を変更するものではない。

## 固定した検証結果

`3e69a78ee8909f7abc2d3f1f5a96eb7a2c736259`の834個のRustファイルを固定し、CI defaultは2,829成功・0失敗・42無効化、179テスト群だった。fmtとfixture hygiene2件を確認し、native/WASMを同じsourceからビルドした。

固定772件は666成功・106拒否で、全出力と拒否結果が確認済み親`293fc19`とRAW一致した。nativeは222成功・30拒否、Chromium/WebKitはそれぞれ222成功・38拒否で、両ブラウザの130 inspectionも親と一致する。成功したことを原本の忠実度の合格とは扱わない。

別枠のW3Cはnative/Chromium/WebKitの通常・strictすべて成功し、6つのDOCXが目視済みの早期候補とRAW一致した。原本の193条件を保持した既知資料の追試であり、7つの青色条件だけを改善した。2つの罫線位置の未達と、本文・見出しの縦位置の問題を残す。

元の90 manifestの延べ94,063ファイル、重複を除く93,285ファイルを再照合した。NTA初回6/62、所有する基線goldの0.05pt、W3C初回181/193と保留中の別候補を変更しない。結果とハッシュは[検証記録](../corpus/validation-opaque-page-group-backgrounds.json)に保存した。限定した不透明なRGBページGroupのセル背景を検証済みとし、全文書の受入れやproduction readinessは未達のままである。他資料の改ページ、表の線と文字、図、対角罫線などの既知の未達を変更しない。
