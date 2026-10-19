//! CCITT不足行・不完全な終端のstrict拒否による保存先保護。
#[path = "../../jade-pdf2docx-parser/tests/support/ccitt_pdf.rs"]
mod fixture;
use jade_pdf2docx_docx::{
    ConvertError, MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx_with_options,
};
use jade_pdf2docx_parser::{ParseError, ParseOptions};

/// 不足行・EOFB欠落の画像は入力・既存出力を保持し、新規出力親も作らない。
#[test]
fn strict_ccitt_invalid_rows_or_terminal_preserves_input_and_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.pdf");
    let existing = directory.path().join("existing.docx");
    std::fs::write(&existing, b"retained output").unwrap();
    // V(0)の白行だけ、または白行とEOFBの前半だけを持つ入力も拒否する。
    let pdfs = [
        fixture::ccitt_pdf(false, 2, &fixture::encoded_rows(false, 1)),
        fixture::ccitt_pdf(true, 2, &fixture::encoded_rows(true, 1)),
        fixture::ccitt_pdf_with_parameters(1, &[0x80], "/K -1"),
        fixture::ccitt_pdf_with_parameters(1, &[0x80, 0x08], "/K -1"),
    ];
    for pdf in pdfs {
        std::fs::write(&input, &pdf).unwrap();
        for output in [existing.clone(), directory.path().join("new/output.docx")] {
            let request = MinimumPdfToDocxRequest {
                input_pdf_path: input.clone(),
                output_docx_path: output,
                ocr_text_lines: None,
                ocr_table: None,
                clean_output: true,
            };
            assert!(matches!(
                convert_minimum_pdf_to_docx_with_options(
                    &request,
                    &ParseOptions {
                        strict: true,
                        ..Default::default()
                    }
                ),
                Err(ConvertError::Parse(ParseError::QualityRejected { .. }))
            ));
        }
        assert_eq!(std::fs::read(&input).unwrap(), pdf);
        assert_eq!(std::fs::read(&existing).unwrap(), b"retained output");
        assert!(!directory.path().join("new").exists());
    }
}
