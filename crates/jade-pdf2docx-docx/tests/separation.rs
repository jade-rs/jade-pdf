//! Separationの色と透明度をDOCX mediaへ保持する。
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/source_image_pdf.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/separation.rs"]
mod separation;

/// 未対応tintのstrict拒否は既存入力と出力を保持する。
#[test]
fn strict_separation_failure_preserves_existing_destination() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("bad-tint.pdf");
    let output = directory.path().join("existing.docx");
    let pdf = separation::pdf(
        &format!("{} /Order 3", separation::LINEAR_RGB),
        "",
        &[64, 255],
    );
    std::fs::write(&input, &pdf).unwrap();
    std::fs::write(&output, b"original output").unwrap();
    let result = jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_options(
        &jade_pdf2docx_docx::MinimumPdfToDocxRequest {
            input_pdf_path: input.clone(),
            output_docx_path: output.clone(),
            ocr_text_lines: None,
            ocr_table: None,
            clean_output: true,
        },
        &jade_pdf2docx_parser::ParseOptions {
            strict: true,
            ..Default::default()
        },
    );
    assert!(matches!(
        result,
        Err(jade_pdf2docx_docx::ConvertError::Parse(
            jade_pdf2docx_parser::ParseError::QualityRejected { .. }
        ))
    ));
    assert_eq!(std::fs::read(input).unwrap(), pdf);
    assert_eq!(std::fs::read(output).unwrap(), b"original output");
}
