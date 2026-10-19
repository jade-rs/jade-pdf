# jade-pdf2docx-ocr-correction

OCR原文を保持し、文書別辞書と明示した文字の取り違え規則から補正候補を返すpure Rust crateです。runtimeの外部依存はありません。

```rust
use jade_pdf2docx_ocr_correction::{CorrectionOptions, Corrector};

let corrector = Corrector::new(
    &["株式会社", "契約書"],
    &[('杜', '社'), ('害', '書')],
    CorrectionOptions::default(),
)?;
let report = corrector.analyze("株式会杜の契約害：100万円", 0.6)?;
assert_eq!(report.candidates().len(), 2);
// 候補を確認して番号を選択する。空の選択なら原文をそのまま返す。
assert_eq!(report.apply_selected(&[0])?, "株式会社の契約害：100万円");
assert_eq!(report.original(), "株式会杜の契約害：100万円");
# Ok::<(), jade_pdf2docx_ocr_correction::CorrectionError>(())
```

`cargo run -p jade-pdf2docx-ocr-correction --example dictionary_review` で、候補と選択適用を確認できます。

既存のOCR結果では、`OcrTextBlock::suggest_corrections(&corrector)` が実認識ブロックの候補を返します。mockとband診断は対象外です。元のブロック・座標・信頼度を変更しません。通常のCLI変換には自動適用しません。

## 候補の扱い

- 既定ではOCR信頼度が0.85未満の行を対象とします。この値は補正の正解確率ではなく、利用側で調整する候補生成の閾値です。
- 辞書語と同じ文字数の範囲を照合し、有向規則にある文字置換だけを許します。既定は一候補につき一文字までです。挿入・削除・Unicode正規化は行いません。
- 完全一致する辞書語に重なる範囲は保護します。ASCII・全角英数字・数字・主要な漢数字・空白・記号を変更する規則は拒否します。
- 候補は原文のUTF-8 byte範囲・原文・置換語・置換数を持ちます。曖昧な候補は併記し、重複選択・重なる範囲の同時選択はエラーにします。
- 入力文字数・辞書語数・語長・規則数・候補数・文字比較回数に上限があります。超過時は部分結果を返さずエラーにします。既定値は `CorrectionLimits` を参照してください。

## 現段階の限界と評価

形態素境界や意味を判断せず部分文字列を照合するため、正しい固有名詞などにも誤った候補を出す可能性があります。原文を保持し、候補の採用は呼び出し側で明示します。辞書と規則の選定も対象文書に合わせて行ってください。

辞書置換と非対象文字の保持は生成入力の単体テスト・Property Based Testingで検査します。旧固定データと実画像の評価結果は削除しました。

本crateについては、実スキャンから認識した原文と人手正解を固定して、CER・誤補正率・候補の採用率を測定することが次の課題です。形態素解析による候補制限、ruri等による文書内用語の取得、再認識との比較はその評価を用意してから追加します。
