use jade_pdf2docx_ocr_correction::{CorrectionOptions, Corrector};

/// 小さな文書別辞書で候補を表示し、一つだけ明示選択した結果を示す。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let corrector = Corrector::new(
        &["株式会社", "契約書"],
        &[('杜', '社'), ('害', '書')],
        CorrectionOptions::default(),
    )?;
    let report = corrector.analyze("株式会杜の契約害：100万円", 0.6)?;
    println!("原文: {}", report.original());
    for (index, candidate) in report.candidates().iter().enumerate() {
        println!(
            "候補{index}: {:?} {} → {}",
            candidate.span, candidate.original, candidate.replacement
        );
    }
    println!("候補0だけを明示選択: {}", report.apply_selected(&[0])?);
    println!("保存した原文: {}", report.original());
    Ok(())
}
