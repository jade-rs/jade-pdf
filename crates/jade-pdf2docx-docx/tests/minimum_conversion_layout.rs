use std::fs;

use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};

mod support;
use support::docx_assert::count_paragraph_tags_in_docx;
use support::paths::temp_file_path;
use support::pdf_builders::{minimal_pdf_with_text, minimal_pdf_with_two_text_objects};

#[test]
fn minimum_conversion_preserves_multiline_preview_in_report() {
    let input_path = temp_file_path("minimum_conversion_multiline", "pdf");
    let output_docx = temp_file_path("minimum_conversion_multiline_out", "docx");
    fs::write(
        &input_path,
        minimal_pdf_with_two_text_objects("LineOne", "LineTwo"),
    )
    .expect("failed to write minimal pdf");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: false,
    };
    let report = convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");
    assert!(
        report.source_text_preview.contains("LineOne\nLineTwo"),
        "report should preserve parser multiline preview"
    );

    // 単一行より複数行の方が段落数が増えることを検証し、
    // preview の改行が DOCX 段落へ反映される仕様を固定する。
    let single_line_input = temp_file_path("minimum_conversion_singleline", "pdf");
    let single_line_output = temp_file_path("minimum_conversion_singleline_out", "docx");
    fs::write(&single_line_input, minimal_pdf_with_text("LineOne"))
        .expect("failed to write single-line pdf");
    let single_req = MinimumPdfToDocxRequest {
        input_pdf_path: single_line_input.clone(),
        output_docx_path: single_line_output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: false,
    };
    convert_minimum_pdf_to_docx(&single_req).expect("single-line conversion must succeed");

    let multi_para_count = count_paragraph_tags_in_docx(&output_docx);
    let single_para_count = count_paragraph_tags_in_docx(&single_line_output);
    assert!(
        multi_para_count > single_para_count,
        "multiline preview should create more paragraphs than single line"
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
    let _ = fs::remove_file(single_line_input);
    let _ = fs::remove_file(single_line_output);
}

#[test]
fn minimum_conversion_reflects_multiline_ocr_input_as_paragraphs() {
    let input_path = temp_file_path("minimum_conversion_ocr_layout", "pdf");
    let output_docx = temp_file_path("minimum_conversion_ocr_layout_out", "docx");
    fs::write(&input_path, minimal_pdf_with_text("ParserLine"))
        .expect("failed to write minimal pdf");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: Some(vec!["OcrA".to_string(), "OcrB".to_string()]),
        ocr_table: None,
        clean_output: false,
    };
    convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");

    let para_count = count_paragraph_tags_in_docx(&output_docx);
    assert!(
        para_count >= 7,
        "title/source/version/byte/label + 2 ocr lines should exist"
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}
