# 原本の表セルの背景

原本の矩形塗りを文字色や罫線と区別し、確認できた範囲だけを編集可能なWordセルの背景へ渡す。表の見出しらしさや数値の内容から色を決めない。

## Rust API

`ParsedTextElement::source_background: Option<ParsedTextBackground>`は、文字を描く前の、未遮蔽の不透明なDevice色矩形を表す。`ParsedTextBackground`は文字と同じページ座標系の`bounds`と、RGB成分の`rgb: [u8; 3]`を持つ。通常の`fill_color`は文字色のままである。

Rustの構造体literalで要素を作る呼び出し側は、未確認なら`source_background: None`を追加する。OCR・nativeだけの未照合span・生成した空白は`None`にする。原本と位置・本文を照合して採用するnative経路は背景もコピーし、別の矩形や色のspanへ流用しない。CropBoxの原点とページ回転は文字と矩形へ同じように適用する。

## 原本で確認する範囲

DeviceGray・RGB・CMYKの有限成分を確認し、既存の8bit RGB変換を使う。CMYKは既存の近似変換であり、ICCによる色の厳密な一致を主張しない。非白・非黒の色があるページだけを候補にする。黒や白を新しいセル背景へ変更せず、後続の白・黒の塗りは遮蔽の検査に使う。

軸平行の単独`re`矩形を、構築時のCTMと塗り命令の色・描画順で保存する。`q/Q`は状態を復元する。複数の矩形を一回で塗るpath、広い非矩形の塗り、曲線塗り、未知のclip、画像やForm、shading、透明groupのあるページでは背景を断定しない。未知の色空間の塗りは遮蔽候補として扱う。狭い既存罫線はセル内部の塗りと区別する。配列・文字列内の偽命令を実行せず、作業配列は確保前に共通のmetadata予算へ予約する。広い矩形が256件を超えるページは推測しない。

単独の軸平行矩形と確認できる`W/W*`は、既存clipとの交差範囲だけを背景の根拠とする。path終端でclipを確定し、空の交差と`q/Q`の復元を区別する。非矩形・曲線・複合pathは未確認のままである。[官報公開ガイドの追試](kanpo_public_guide_holdout.md)に全セル被覆、部分clipの否定例と残る文字切れを記録した。

所属ページにDevice色の既定置換がなく、全ExtGStateが既知で、不透明な通常合成・soft maskなしである場合に限る。文字のfontは非Type3の種別を確認する。不明なfontや独自glyph program、Form内の文字から背景を推測しない。

文字より先に塗られた同色の矩形のうち、文字全体を含む最大の範囲を使う。後続の別色がその範囲と重なる場合、または文字を後から塗りが覆う場合は未確認へ戻す。同色の内側矩形を重ねただけで、元の外側範囲を小さくしない。

## Wordのセルとの対応

原本gridでセルと全文を照合した経路だけへ適用する。全ての非空白文字が同じ確認済み色を持ち、各矩形が結合セル全体を覆うことを要求する。中心線で表す原本罫線の半幅と整数gridの丸めに限って端点差を許し、最大2ptで打ち切る。単なる部分塗りをセル全体へ広げない。

空セルでは、同じページで検証した原本矩形がセル全体を覆う場合だけ色を使う。隣のセルだけを塗る矩形は流用しない。通常の段落、OCR表、罫線のない反復列の表へ、この背景を推測で広げない。

## 確認中の資料

労働力調査の[固定した原本57条件と初回結果](source_white_fill_and_adjacent_tables.md)、文字強調の[直前の検証記録](source_painted_text_emphasis.md)を使う。原本・初回結果・goldを変更せず、既知資料への追試と扱う。

native試作では青い11セルと9月の`2.4`の黄色1セルを確認し、背景のshadingだけを除くと親の本文XMLと一致した。他のZIP部品もバイト一致し、非空白1,164文字・4表・4ページを保持した。変更した1ページ目を原本と目視し、残り3ページは目視済み親出力とRGB一致した。最初の部分的な色回復だけの試作は保留とし、承認済みの基準にしない。

原本2ページがWord4ページになる問題、graph、font face、logo、余分な表、clippingなどは残る。ここまでは試作時の確認であり、固定した最終結果を次節に記録する。`whole_document_accepted=false`と`production_ready=false`を維持する。

kanpoの表とstyleを原本の所有へ結び付ける考え方は[固定した参照記録](source_typography_and_table_opening.md#参照)を参照する。custom licenseのコードや依存は取り込まず、既存のRust parserとWord builderへ独立に実装する。

## 固定した最終検証

実装コミット`a72307e63e9b23d5456eaf59ff80342f57724be6`を固定し、Rust1.93.1のCI defaultは2,651成功・0失敗・42無効化、157テスト群だった。789個のRustファイルは検証・native/WASMビルドを通して同一である。fmt、fixture hygiene2件、WASM distribution10件も通過した。

公開回帰10件と、平行移動24例のPBT、追加したparser予算境界2件を含む。最初の色消失、罫線内側と丸め、空セル、page group、不明fontの失敗ログを保持した。隣の通常セル、別色や後続の遮蔽、部分塗り、未知の合成、Type3の否定例を確認した。

native194件とChromium/WebKit各202件、合計598件は直前の固定済み出力と全ての成否が一致した。595件はpayloadがバイト一致し、変化は既知の労働力調査の通常変換3件だけだった。3環境のDOCXは目視した試作とRAW一致し、両ブラウザは202 payloadと101 inspectionが一致する。CPIを含む他資料の元の未達を変更しない。

原本57条件の追試は、各環境で34合格・23未達/未確認となった。直前32/25から追加合格したのは、両表の青い見出しと9月2.4の黄色セルの2条件だけである。初回20/37、表の改善31/26、文字強調32/25は履歴として不変にする。未見文書の精度率とは扱わない。原本2→Word4ページ、graph・font・logo・余分な表・clipping・原本座標との対応は依然として未達である。

原本・初回結果・gold・保留試作の75 manifest、延べ52,870ファイルを再照合した。ハッシュと範囲は[`corpus/validation-source-table-cell-backgrounds.json`](../corpus/validation-source-table-cell-backgrounds.json)に記録する。限定したセル背景の確認であり、`whole_document_accepted=false`、`production_ready=false`を維持する。
