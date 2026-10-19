//! parserの共通確保上限がDOCX保存前に伝播することを検証する。
#[path = "support/pdf_builders.rs"]
mod fixture;
use jade_pdf2docx_docx::{
    ConvertError, MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx_with_options,
};
use jade_pdf2docx_parser::{ParseError, ParseLimits, ParseOptions};

/// 資源超過時はstrict指定に関係なく入力・既存出力・未作成の親を維持する。
#[test]
fn allocation_failure_preserves_output_before_publication() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.pdf");
    let pdf = fixture::minimal_pdf_with_text("allocation budget");
    std::fs::write(&input, &pdf).unwrap();
    let existing = directory.path().join("existing.docx");
    std::fs::write(&existing, b"retained output").unwrap();
    for strict in [false, true] {
        for output in [existing.clone(), directory.path().join("new/output.docx")] {
            let request = MinimumPdfToDocxRequest {
                input_pdf_path: input.clone(),
                output_docx_path: output,
                ocr_text_lines: None,
                ocr_table: None,
                clean_output: true,
            };
            let options = ParseOptions {
                limits: ParseLimits {
                    max_total_allocation_bytes: 0,
                    ..Default::default()
                },
                strict,
                ..Default::default()
            };
            assert!(matches!(
                convert_minimum_pdf_to_docx_with_options(&request, &options),
                Err(ConvertError::Parse(ParseError::ResourceLimit {
                    resource: "total allocation bytes",
                    limit: 0,
                    ..
                }))
            ));
        }
    }
    assert_eq!(std::fs::read(input).unwrap(), pdf.as_bytes());
    assert_eq!(std::fs::read(existing).unwrap(), b"retained output");
    assert!(!directory.path().join("new").exists());
}
