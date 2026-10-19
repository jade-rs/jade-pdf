use std::{
    fs,
    io::{Read, Write},
};

use fax::{
    BitWriter, Bits, Color, VecWriter,
    encoder::Encoder,
    maps::{EOL, black, white},
};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use jade_pdf2docx_docx::{
    AutoOcrPolicy, MinimumPdfToDocxRequest, RenderedPageImage, automatic_ocr_pdf_page_indices,
    automatic_ocr_pdf_page_indices_with_policy, convert_minimum_pdf_bytes_to_docx_bytes,
    convert_minimum_pdf_to_docx, convert_rendered_png_pages_to_docx_bytes,
};
use jade_pdf2docx_ocr::{OcrCellSpan, OcrTable};
use jade_pdf2docx_parser::{
    ParsedBounds, ParsedDocument, ParsedGraphicElement, ParsedImageElement, ParsedPage,
    ParsedTextElement, PdfVersion, parse_pdf,
};

mod support;
use support::docx_assert::{
    count_grid_span_tags_in_docx, count_media_files_in_docx, count_page_breaks_in_docx,
    count_table_cell_tags_in_docx, count_table_tags_in_docx, count_vertical_merge_tags_in_docx,
    paragraph_texts_in_docx, read_document_relationships_xml, read_document_xml, read_docx_entry,
    read_first_media_file_in_docx, text_nodes_in_docx,
};
use support::paths::temp_file_path;
use support::pdf_builders::{minimal_pdf_with_text, minimal_pdf_without_text};

/// 空白のみを保存するparserでも、本文のないPDFは従来のfallbackで変換する。
#[test]
fn whitespace_only_layer_keeps_empty_document_fallback() {
    let pdf = minimal_pdf_with_text("   ");
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("spaces.pdf", pdf.as_bytes()).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("(no extractable content)"), "{xml}");
}

#[test]
fn automatic_ocr_targets_image_only_pages() {
    let parsed =
        parsed_document_for_auto_ocr(1, Vec::new(), vec![image_element_on_page(0)], String::new());

    assert_eq!(automatic_ocr_pdf_page_indices(&parsed), vec![0]);
}

#[test]
fn automatic_ocr_does_not_target_image_on_text_pdf_page() {
    let parsed = parsed_document_for_auto_ocr(
        1,
        vec![text_element_on_page(
            0,
            0.0,
            0.0,
            "このページには十分なテキスト情報があります",
        )],
        vec![image_element_on_page(0)],
        "このページには十分なテキスト情報があります".to_string(),
    );

    assert!(
        automatic_ocr_pdf_page_indices(&parsed).is_empty(),
        "text-layer pages must not OCR embedded images"
    );
}

#[test]
fn automatic_ocr_targets_only_mixed_pdf_pages_without_text_layer() {
    let parsed = parsed_document_for_auto_ocr(
        3,
        vec![
            text_element_on_page(0, 0.0, 0.0, "page one text"),
            text_element_on_page(2, 0.0, 0.0, "page three text"),
        ],
        vec![
            image_element_on_page(0),
            image_element_on_page(1),
            image_element_on_page(2),
        ],
        "page one text\npage three text".to_string(),
    );

    assert_eq!(automatic_ocr_pdf_page_indices(&parsed), vec![1]);
}

#[test]
fn automatic_ocr_policy_can_disable_image_requirement_for_library_callers() {
    let parsed = parsed_document_for_auto_ocr(1, Vec::new(), Vec::new(), String::new());

    let pages = automatic_ocr_pdf_page_indices_with_policy(
        &parsed,
        AutoOcrPolicy {
            require_page_image: false,
            ..AutoOcrPolicy::default()
        },
    );

    assert_eq!(pages, vec![0]);
}

#[test]
fn minimum_conversion_removes_xml_invalid_control_chars_from_text_nodes() {
    let input_pdf = temp_file_path("minimum_conversion_invalid_xml_text", "pdf");
    let output_docx = temp_file_path("minimum_conversion_invalid_xml_text", "docx");
    fs::write(&input_pdf, minimal_pdf_without_text()).expect("failed to write input pdf");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_pdf.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: Some(vec!["A\u{0}B\u{8}C".to_string()]),
        ocr_table: None,
        clean_output: true,
    };
    convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        !doc_xml.contains('\u{0}') && !doc_xml.contains('\u{8}'),
        "DOCX XML text nodes must not contain XML-invalid control characters"
    );
    assert!(doc_xml.contains("ABC"));

    let _ = fs::remove_file(input_pdf);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_rejects_non_pdf_input() {
    let input_path = temp_file_path("minimum_conversion_invalid", "bin");
    let output_docx = temp_file_path("minimum_conversion_invalid_out", "docx");
    fs::write(&input_path, b"NOT A PDF").expect("failed to write invalid input");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: false,
    };
    let result = convert_minimum_pdf_to_docx(&request);
    assert!(result.is_err(), "non-pdf input must fail");

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_includes_text_preview() {
    let input_path = temp_file_path("minimum_conversion_text", "pdf");
    let output_docx = temp_file_path("minimum_conversion_text_out", "docx");
    fs::write(&input_path, minimal_pdf_with_text("Hello DOCX TDD"))
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
        report.source_text_preview.contains("Hello DOCX TDD"),
        "text preview must contain literal text"
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_uses_fallback_when_text_preview_is_empty() {
    let input_path = temp_file_path("minimum_conversion_no_text", "pdf");
    let output_docx = temp_file_path("minimum_conversion_no_text_out", "docx");
    fs::write(&input_path, minimal_pdf_without_text()).expect("failed to write minimal pdf");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: false,
    };
    let report = convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");
    assert_eq!(report.source_text_preview, "(no text extracted)");

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_prefers_ocr_text_lines_when_provided() {
    let input_path = temp_file_path("minimum_conversion_ocr", "pdf");
    let output_docx = temp_file_path("minimum_conversion_ocr_out", "docx");
    fs::write(&input_path, minimal_pdf_without_text()).expect("failed to write minimal pdf");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: Some(vec![
            " OCR Line One ".to_string(),
            "".to_string(),
            "OCR Line Two".to_string(),
        ]),
        ocr_table: None,
        clean_output: false,
    };
    let report = convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");
    assert_eq!(report.source_text_preview, "OCR Line One\nOCR Line Two");

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_joins_unintended_cjk_ocr_line_breaks() {
    let input_path = temp_file_path("minimum_conversion_ocr_join", "pdf");
    let output_docx = temp_file_path("minimum_conversion_ocr_join_out", "docx");
    fs::write(&input_path, minimal_pdf_without_text()).expect("failed to write minimal pdf");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: Some(vec![
            "これはOCR結果から".to_string(),
            "意図しない改行が".to_string(),
            "入る場合です。".to_string(),
            "（1）別の項目です".to_string(),
        ]),
        ocr_table: None,
        clean_output: true,
    };
    let report = convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");

    assert_eq!(
        report.source_text_preview,
        "これはOCR結果から意図しない改行が入る場合です。\n（1）別の項目です"
    );
    assert_eq!(
        paragraph_texts_in_docx(&output_docx),
        vec![
            "これはOCR結果から意図しない改行が入る場合です。".to_string(),
            "（1）別の項目です".to_string(),
        ]
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_splits_and_joins_embedded_ocr_newlines() {
    let input_path = temp_file_path("minimum_conversion_ocr_embedded_join", "pdf");
    let output_docx = temp_file_path("minimum_conversion_ocr_embedded_join_out", "docx");
    fs::write(&input_path, minimal_pdf_without_text()).expect("failed to write minimal pdf");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: Some(vec![
            "これはOCR結果から\n意図しない改行が".to_string(),
            "入る場合です。\r\n次の段落です。".to_string(),
        ]),
        ocr_table: None,
        clean_output: true,
    };
    let report = convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");

    assert_eq!(
        report.source_text_preview,
        "これはOCR結果から意図しない改行が入る場合です。\n次の段落です。"
    );
    assert_eq!(
        paragraph_texts_in_docx(&output_docx),
        vec![
            "これはOCR結果から意図しない改行が入る場合です。".to_string(),
            "次の段落です。".to_string(),
        ]
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_emits_docx_table_when_ocr_table_provided() {
    let input_path = temp_file_path("minimum_conversion_table", "pdf");
    let output_docx = temp_file_path("minimum_conversion_table_out", "docx");
    fs::write(&input_path, minimal_pdf_with_text("ignored for preview"))
        .expect("failed to write minimal pdf");

    let table = OcrTable::new(vec![
        vec!["A1".to_string(), "B1".to_string()],
        vec!["A2".to_string(), "B2".to_string()],
    ]);
    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: Some(table),
        clean_output: false,
    };
    convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");

    assert_eq!(
        count_table_tags_in_docx(&output_docx),
        1,
        "one w:tbl must be present when ocr_table is set"
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_emits_2x2_table_cell_texts_in_document_xml() {
    let input_path = temp_file_path("minimum_conversion_table_xml", "pdf");
    let output_docx = temp_file_path("minimum_conversion_table_xml_out", "docx");
    fs::write(&input_path, minimal_pdf_without_text()).expect("failed to write minimal pdf");

    let table = OcrTable::new(vec![
        vec!["A1".to_string(), "B1".to_string()],
        vec!["A2".to_string(), "B2".to_string()],
    ]);
    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: Some(table),
        clean_output: true,
    };
    convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_table_cell_tags_in_docx(&output_docx), 4);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "A1".to_string(),
            "B1".to_string(),
            "A2".to_string(),
            "B2".to_string(),
        ]
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn minimum_conversion_emits_public_ocr_table_spans_in_document_xml() {
    let input_path = temp_file_path("minimum_conversion_table_spans", "pdf");
    let output_docx = temp_file_path("minimum_conversion_table_spans_out", "docx");
    fs::write(&input_path, minimal_pdf_without_text()).expect("failed to write minimal pdf");

    let table = OcrTable::with_spans(
        vec![
            vec!["Title".to_string(), "".to_string()],
            vec!["A".to_string(), "B".to_string()],
            vec!["Merged".to_string(), "C".to_string()],
            vec!["".to_string(), "D".to_string()],
        ],
        vec![
            vec![OcrCellSpan::new(1, 2), OcrCellSpan::single()],
            vec![OcrCellSpan::single(), OcrCellSpan::single()],
            vec![OcrCellSpan::new(2, 1), OcrCellSpan::single()],
            vec![OcrCellSpan::single(), OcrCellSpan::single()],
        ],
    );
    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: Some(table),
        clean_output: true,
    };
    convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_grid_span_tags_in_docx(&output_docx), 1);
    assert_eq!(count_vertical_merge_tags_in_docx(&output_docx), 2);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Title".to_string(),
            "A".to_string(),
            "B".to_string(),
            "Merged".to_string(),
            "C".to_string(),
            "".to_string(),
            "D".to_string(),
        ]
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_infers_table_when_inner_vertical_rule_is_missing() {
    let content = [
        b"20 150 m 120 150 l S 20 120 m 120 120 l S 20 90 m 120 90 l S".as_slice(),
        b"20 90 m 20 150 l S 120 90 m 120 150 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 132 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 132 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 102 Tm (A2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 102 Tm (B2) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_missing_inner_rule", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("missing-inner-rule.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_table_cell_tags_in_docx(&output_docx), 4);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "A1".to_string(),
            "B1".to_string(),
            "A2".to_string(),
            "B2".to_string(),
        ]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_emits_grid_span_when_vertical_rule_is_missing_in_row_band() {
    let content = [
        b"20 150 m 120 150 l S 20 120 m 120 120 l S 20 90 m 120 90 l S".as_slice(),
        b"20 90 m 20 150 l S 70 90 m 70 120 l S 120 90 m 120 150 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 55 132 Tm (Title) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 102 Tm (A2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 102 Tm (B2) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_grid_span", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("grid-span.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_table_cell_tags_in_docx(&output_docx), 3);
    assert_eq!(count_grid_span_tags_in_docx(&output_docx), 1);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec!["Title".to_string(), "A2".to_string(), "B2".to_string()]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_emits_vertical_merge_when_horizontal_rule_is_missing_in_column_band() {
    let content = [
        b"20 150 m 120 150 l S 70 120 m 120 120 l S 20 90 m 120 90 l S".as_slice(),
        b"20 90 m 20 150 l S 70 90 m 70 150 l S 120 90 m 120 150 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 125 Tm (Merged) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 132 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 102 Tm (B2) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_vertical_merge", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("vertical-merge.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_table_cell_tags_in_docx(&output_docx), 4);
    assert_eq!(count_vertical_merge_tags_in_docx(&output_docx), 2);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Merged".to_string(),
            "B1".to_string(),
            "".to_string(),
            "B2".to_string(),
        ]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_infers_table_when_outer_border_is_missing() {
    let content = [
        b"20 120 m 120 120 l S 20 90 m 120 90 l S 70 60 m 70 150 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 132 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 132 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 102 Tm (A2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 102 Tm (B2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 72 Tm (A3) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 72 Tm (B3) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_outer_border_missing", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("outer-border-missing.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_table_cell_tags_in_docx(&output_docx), 6);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "A1".to_string(),
            "B1".to_string(),
            "A2".to_string(),
            "B2".to_string(),
            "A3".to_string(),
            "B3".to_string(),
        ]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_infers_table_from_nearly_axis_aligned_thin_rules() {
    let content = [
        b"20 120.2 m 120 120 l S 20 90 m 120 90.2 l S 70.1 60 m 70 150 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 132 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 132 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 102 Tm (A2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 102 Tm (B2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 72 Tm (A3) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 72 Tm (B3) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_nearly_axis_aligned_rules", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("nearly-axis-aligned-rules.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_table_cell_tags_in_docx(&output_docx), 6);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_infers_table_from_two_point_filled_rule_rectangles() {
    let content = [
        b"20 149 100 2 re f 20 119 100 2 re f 20 89 100 2 re f 20 59 100 2 re f".as_slice(),
        b"19 60 2 90 re f 69 60 2 90 re f 119 60 2 90 re f".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 132 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 132 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 102 Tm (A2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 102 Tm (B2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 72 Tm (A3) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 72 Tm (B3) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_two_point_filled_rules", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("two-point-filled-rules.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_table_cell_tags_in_docx(&output_docx), 6);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "A1".to_string(),
            "B1".to_string(),
            "A2".to_string(),
            "B2".to_string(),
            "A3".to_string(),
            "B3".to_string(),
        ]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_infers_two_row_table_when_outer_border_is_missing() {
    let content = [
        b"20 90 m 120 90 l S 70 60 m 70 120 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 102 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 102 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 72 Tm (A2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 72 Tm (B2) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_two_row_outer_border_missing", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("two-row-outer-border-missing.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(count_table_cell_tags_in_docx(&output_docx), 4);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "A1".to_string(),
            "B1".to_string(),
            "A2".to_string(),
            "B2".to_string(),
        ]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_embeds_detected_pdf_image_xobject_as_drawing() {
    let pdf_bytes = minimal_pdf_with_image_xobject();
    let output_docx = temp_file_path("minimum_conversion_pdf_image_xobject", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("image-xobject.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_media_files_in_docx(&output_docx), 1);
    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert_eq!(doc_xml.matches("<w:drawing>").count(), 1);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_ignores_unplaced_pdf_image_xobject() {
    let pdf_bytes = minimal_pdf_with_unplaced_image_xobject();
    let output_docx = temp_file_path("minimum_conversion_unplaced_image_xobject", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("unplaced-image.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        count_media_files_in_docx(&output_docx),
        1,
        "painted image XObjects must remain, but unused sibling image resources must be ignored"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_decodes_flate_device_rgb_image_xobject_as_png() {
    let pdf_bytes = minimal_pdf_with_flate_rgb_image_xobject();
    let output_docx = temp_file_path("minimum_conversion_pdf_decoded_image_xobject", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("decoded-image-xobject.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 2);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 1);

    let _ = fs::remove_file(output_docx);
}

/// 画素不足の大型画像はdecode失敗として診断し、代替画像も元寸法を保持する。
#[test]
fn byte_conversion_diagnoses_invalid_large_flate_image_xobject() {
    let image_stream = zlib_compress(&[255, 0, 0]);
    let pdf_bytes =
        minimal_pdf_with_flate_image_xobject(&image_stream, 2001, 2000, "/DeviceRGB", "");
    let output_docx = temp_file_path("minimum_conversion_oversized_flate_image_xobject", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("oversized-image-xobject.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 2001);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 2000);
    let parsed = jade_pdf2docx_parser::parse_pdf_bytes("invalid-large.pdf", &pdf_bytes).unwrap();
    assert!(parsed.quality.issues.iter().any(|issue| matches!(
        issue,
        jade_pdf2docx_parser::QualityIssue::ImageReplaced {
            reason: jade_pdf2docx_parser::ImageReplacementReason::DecodeFailed,
            ..
        }
    )));

    let _ = fs::remove_file(output_docx);
}

/// 21枚の正常Flate画像も元寸法でDOCXへ保存する。
#[test]
fn byte_conversion_preserves_many_flate_image_xobjects() {
    let pdf_bytes = minimal_pdf_with_many_flate_image_xobjects(21);
    let output_docx = temp_file_path("minimum_conversion_many_flate_image_xobjects", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("many-image-xobjects.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert!(
        count_media_files_in_docx(&output_docx) >= 1,
        "at least one placed image must remain in the DOCX"
    );
    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 2);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 1);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_decodes_flate_device_gray_image_xobject_as_png() {
    let image_stream = zlib_compress(&[0, 255]);
    let pdf_bytes = minimal_pdf_with_flate_image_xobject(&image_stream, 2, 1, "/DeviceGray", "");
    let output_docx = temp_file_path("minimum_conversion_pdf_gray_image_xobject", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("gray-image-xobject.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 2);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 1);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_decodes_flate_indexed_image_xobject_as_png() {
    let image_stream = zlib_compress(&[0, 1]);
    let pdf_bytes = minimal_pdf_with_flate_image_xobject(
        &image_stream,
        2,
        1,
        "[/Indexed /DeviceRGB 1 <ff00000000ff>]",
        "",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_indexed_image_xobject", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("indexed-image-xobject.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 2);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 1);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_applies_flate_smask_to_image_xobject_png() {
    let rgb_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    let mask_stream = zlib_compress(&[0, 255]);
    let pdf_bytes = minimal_pdf_with_flate_smask_image_xobject(&rgb_stream, &mask_stream, 2, 1);
    let output_docx = temp_file_path("minimum_conversion_pdf_smask_image_xobject", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("smask-image-xobject.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 2);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 1);
    assert_eq!(media[25], 6, "SMask output must be RGBA PNG");

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_resizes_image_drawing_from_pdf_ctm_bounds() {
    let image_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    let pdf_bytes = minimal_pdf_with_flate_image_xobject_and_content(
        &image_stream,
        2,
        1,
        "/DeviceRGB",
        "",
        b"q 50 0 0 40 20 30 cm /Im1 Do Q",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_ctm_image_size", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("ctm-image-size.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"cx="638175""#),
        "expected 67px/638175emu width from 50pt CTM; XML: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"cy="504825""#),
        "expected 53px/504825emu height from 40pt CTM; XML: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_floats_and_rotates_image_drawing_from_rotated_ctm() {
    let image_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    let pdf_bytes = minimal_pdf_with_flate_image_xobject_and_content(
        &image_stream,
        2,
        1,
        "/DeviceRGB",
        "",
        b"q 0 40 -50 0 70 30 cm /Im1 Do Q",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_rotated_ctm_image", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("rotated-ctm-image.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert!(
        doc_xml.contains("<wp:anchor"),
        "rotated image must be floating"
    );
    assert!(
        doc_xml.contains(r#"rot="5400000""#),
        "90 degree rotation must be preserved; XML: {doc_xml}"
    );
    assert!(
        doc_xml.contains("<wp:posOffset>254000</wp:posOffset>"),
        "x offset from CTM bounds must be preserved; XML: {doc_xml}"
    );
    assert!(
        doc_xml.contains("<wp:posOffset>381000</wp:posOffset>"),
        "y offset from CTM bounds must be preserved; XML: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_floats_image_drawing_from_skewed_ctm() {
    let image_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    let pdf_bytes = minimal_pdf_with_flate_image_xobject_and_content(
        &image_stream,
        2,
        1,
        "/DeviceRGB",
        "",
        b"q 50 0 20 40 20 30 cm /Im1 Do Q",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_skewed_ctm_image", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("skewed-ctm-image.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert!(
        doc_xml.contains("<wp:anchor"),
        "skewed image must be floating; XML: {doc_xml}"
    );
    assert!(
        doc_xml.contains("<wp:posOffset>254000</wp:posOffset>"),
        "x offset from skewed CTM bounds must be preserved; XML: {doc_xml}"
    );
    assert!(
        doc_xml.contains("<wp:posOffset>381000</wp:posOffset>"),
        "y offset from skewed CTM bounds must be preserved; XML: {doc_xml}"
    );
    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(
        u32::from_be_bytes(media[16..20].try_into().unwrap()),
        93,
        "skewed CTM must be baked into a bounding-box-width PNG"
    );
    assert_eq!(
        u32::from_be_bytes(media[20..24].try_into().unwrap()),
        53,
        "skewed CTM must be baked into a bounding-box-height PNG"
    );
    assert_eq!(
        media[25], 6,
        "skewed CTM output must include transparent corners"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_preserves_non_right_angle_image_rotation_from_ctm() {
    let image_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    let pdf_bytes = minimal_pdf_with_flate_image_xobject_and_content(
        &image_stream,
        2,
        1,
        "/DeviceRGB",
        "",
        b"q 49.24 8.68 -8.68 49.24 20 30 cm /Im1 Do Q",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_ten_degree_ctm_image", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("ten-degree-ctm-image.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert!(
        doc_xml.contains("<wp:anchor"),
        "non-right-angle rotated image must be floating; XML: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"rot="600000""#),
        "10 degree rotation must be preserved; XML: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_decodes_indexed_image_with_indirect_palette() {
    let image_stream = zlib_compress(&[0, 1]);
    let pdf_bytes = minimal_pdf_with_flate_image_xobject(
        &image_stream,
        2,
        1,
        "[/Indexed /DeviceRGB 1 6 0 R]",
        "",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_indirect_palette", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("indirect-palette-image.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 2);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 1);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_resamples_smaller_smask_to_image_size() {
    let rgb_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    let mask_stream = zlib_compress(&[128]);
    let pdf_bytes = minimal_pdf_with_flate_smask_image_xobject_with_mask_size(
        &rgb_stream,
        &mask_stream,
        2,
        1,
        1,
        1,
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_resampled_smask", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("resampled-smask-image.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(media[25], 6, "resampled SMask output must be RGBA PNG");

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_decodes_smask_with_indirect_device_gray_colorspace() {
    let rgb_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    let mask_stream = zlib_compress(&[0, 255]);
    let pdf_bytes = minimal_pdf_with_flate_smask_image_xobject_with_mask_colorspace_ref(
        &rgb_stream,
        &mask_stream,
        2,
        1,
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_indirect_smask_colorspace", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("indirect-smask-colorspace.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(
        media[25], 6,
        "indirect SMask ColorSpace output must be RGBA PNG"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_keeps_ccitt_and_jbig2_images_as_drawings_with_fallback_png() {
    let pdf_bytes =
        minimal_pdf_with_raw_image_xobject(&[0xff], 8, 1, "/DeviceGray", "/JBIG2Decode");
    let output_docx = temp_file_path("jbig2-image-xobject.pdf", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("jbig2-image-xobject.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_media_files_in_docx(&output_docx), 1);
    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 8);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 1);
    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert_eq!(doc_xml.matches("<w:drawing>").count(), 1);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_decodes_ccitt_group4_image_xobject_as_png() {
    let image_stream = ccitt_group4_stream_from_rows(&[
        &[false, false, true, true, true, true, false, false],
        &[true, true, false, false, false, false, true, true],
    ]);
    let pdf_bytes = minimal_pdf_with_raw_image_xobject(
        &image_stream,
        8,
        2,
        "/DeviceGray",
        "/CCITTFaxDecode /DecodeParms << /K -1 /Columns 8 /Rows 2 /BlackIs1 true >>",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_ccitt_group4", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("ccitt-group4-image.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 8);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 2);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_decodes_ccitt_group3_1d_image_xobject_as_png() {
    let image_stream = ccitt_group3_1d_stream_from_rows(&[
        &[false, false, true, true, true, true, false, false],
        &[true, true, false, false, false, false, true, true],
    ]);
    let pdf_bytes = minimal_pdf_with_raw_image_xobject(
        &image_stream,
        8,
        2,
        "/DeviceGray",
        "/CCITTFaxDecode /DecodeParms << /K 0 /Columns 8 /Rows 2 /BlackIs1 true >>",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_ccitt_group3_1d", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("ccitt-group3-1d-image.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 8);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 2);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_decodes_ccitt_group3_byte_aligned_eol_image_xobject_as_png() {
    let image_stream = ccitt_group3_1d_byte_aligned_stream_from_rows(&[
        &[false, false, true, true, true, true, false, false],
        &[true, true, false, false, false, false, true, true],
    ]);
    let pdf_bytes = minimal_pdf_with_raw_image_xobject(
        &image_stream,
        8,
        2,
        "/DeviceGray",
        "/CCITTFaxDecode /DecodeParms << /K 0 /Columns 8 /Rows 2 /BlackIs1 true /EndOfLine true /EncodedByteAlign true >>",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_ccitt_group3_byte_aligned", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("ccitt-group3-byte-aligned-image.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 8);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 2);
    assert!(
        png_rgb_has_non_white_pixel(&media),
        "byte-aligned CCITT Group3 stream must preserve black pixels"
    );

    let _ = fs::remove_file(output_docx);
}

/// 未対応の混合方式をGroup 4として復号せず、寸法を保持する白画像へ置換する。
#[test]
fn byte_conversion_replaces_unsupported_ccitt_mixed_mode_with_white_image() {
    let image_stream = ccitt_group4_stream_from_rows(&[
        &[false, false, true, true, true, true, false, false],
        &[true, true, false, false, false, false, true, true],
    ]);
    let pdf_bytes = minimal_pdf_with_raw_image_xobject(
        &image_stream,
        8,
        2,
        "/DeviceGray",
        "/CCITTFaxDecode /DecodeParms << /K 1 /Columns 8 /Rows 2 /BlackIs1 true >>",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_ccitt_mixed_2d", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("ccitt-mixed-2d-image.pdf", &pdf_bytes)
            .expect("non-strict conversion must retain the white replacement");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let media = read_first_media_file_in_docx(&output_docx);
    assert_eq!(&media[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(media[16..20].try_into().unwrap()), 8);
    assert_eq!(u32::from_be_bytes(media[20..24].try_into().unwrap()), 2);
    assert!(
        !png_rgb_has_non_white_pixel(&media),
        "unsupported mixed CCITT data must not be decoded as Group 4"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_uses_pdf_coordinates_for_paragraph_order_and_line_joining() {
    let pdf_bytes = flate_pdf_with_page_contents(&[&[
        b"BT /F1 10 Tf 1 0 0 1 20 100 Tm (Bottom) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Top) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 42 150 Tm (Line) Tj ET".as_slice(),
    ]
    .concat()]);
    let output_docx = temp_file_path("minimum_conversion_coordinate_order", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("coordinate-order.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        paragraph_texts_in_docx(&output_docx),
        vec!["TopLine".to_string(), "Bottom".to_string()]
    );

    let _ = fs::remove_file(output_docx);
}

/// PDF 本文行の間にある縦方向の空きを DOCX paragraph spacing として保持する。
#[test]
fn byte_conversion_preserves_pdf_vertical_gap_between_paragraphs() {
    let pdf_bytes = flate_pdf_with_page_contents(&[&[
        b"BT /F1 10 Tf 1 0 0 1 20 170 Tm (First line) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 100 Tm (Second line) Tj ET".as_slice(),
    ]
    .concat()]);
    let output_docx = temp_file_path("minimum_conversion_vertical_paragraph_gap", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("vertical-gap.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let document_xml = read_document_xml(&output_docx);
    assert!(
        document_xml.contains(r#"<w:spacing w:before="1200" w:after="0""#),
        "60pt PDF blank gap between 10pt lines must become 1200twips paragraph spacing: {document_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_ignores_pdf_footer_page_numbers() {
    let pdf_bytes = flate_pdf_with_page_contents(&[&[
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Body text) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 96 12 Tm (-) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 102 12 Tm (1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 108 12 Tm (-) Tj ET".as_slice(),
    ]
    .concat()]);
    let output_docx = temp_file_path("minimum_conversion_ignores_page_numbers", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("page-number-footer.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        paragraph_texts_in_docx(&output_docx),
        vec!["Body text".to_string()]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_moves_repeated_pdf_header_and_footer_to_docx_parts() {
    let page_one = [
        b"BT /F1 10 Tf 1 0 0 1 20 190 Tm (Shared Header) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 100 Tm (Page one body) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 10 Tm (Shared Footer) Tj ET".as_slice(),
    ]
    .concat();
    let page_two = [
        b"BT /F1 10 Tf 1 0 0 1 20 190 Tm (Shared Header) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 100 Tm (Page two body) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 10 Tm (Shared Footer) Tj ET".as_slice(),
    ]
    .concat();
    let pdf_bytes = flate_pdf_with_page_contents(&[&page_one, &page_two]);
    let output_docx = temp_file_path("minimum_conversion_repeated_chrome", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("repeated-chrome.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        paragraph_texts_in_docx(&output_docx),
        vec!["Page one body".to_string(), "Page two body".to_string()]
    );
    assert!(read_document_xml(&output_docx).contains("<w:headerReference"));
    assert!(read_document_xml(&output_docx).contains("<w:footerReference"));
    assert!(read_docx_entry(&output_docx, "word/header1.xml").contains("Shared Header"));
    assert!(read_docx_entry(&output_docx, "word/footer1.xml").contains("Shared Footer"));
    assert!(
        read_document_relationships_xml(&output_docx).contains("header1.xml")
            && read_document_relationships_xml(&output_docx).contains("footer1.xml")
    );

    let _ = fs::remove_file(output_docx);
}

/// 反復 header/footer の PDF 配置と文字 style を DOCX part へ保持する。
#[test]
fn byte_conversion_preserves_repeated_pdf_chrome_alignment_and_text_style() {
    let page_one = [
        b"BT /F2 14 Tf 1 0 0 1 120 175 Tm (Shared Header) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 100 Tm (Page one body) Tj ET".as_slice(),
        b"BT /F3 9 Tf 1 0 0 1 230 10 Tm (Shared Footer) Tj ET".as_slice(),
    ]
    .concat();
    let page_two = [
        b"BT /F2 14 Tf 1 0 0 1 120 175 Tm (Shared Header) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 100 Tm (Page two body) Tj ET".as_slice(),
        b"BT /F3 9 Tf 1 0 0 1 230 10 Tm (Shared Footer) Tj ET".as_slice(),
    ]
    .concat();
    let pdf_bytes = flate_pdf_with_page_contents_and_base_fonts(
        &[&page_one, &page_two],
        &[
            ("F1", "Helvetica"),
            ("F2", "Helvetica-Bold"),
            ("F3", "Helvetica-Oblique"),
        ],
    );
    let output_docx = temp_file_path("minimum_conversion_styled_repeated_chrome", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("styled-repeated-chrome.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let header_xml = read_docx_entry(&output_docx, "word/header1.xml");
    assert!(
        header_xml.contains(r#"<w:jc w:val="center""#),
        "centered PDF header must remain centered: {header_xml}"
    );
    assert!(
        header_xml.contains(r#"<w:sz w:val="28""#) && header_xml.contains("<w:b"),
        "14pt bold PDF header must keep its text style: {header_xml}"
    );
    let footer_xml = read_docx_entry(&output_docx, "word/footer1.xml");
    assert!(
        footer_xml.contains(r#"<w:jc w:val="right""#),
        "right-aligned PDF footer must remain right-aligned: {footer_xml}"
    );
    assert!(
        footer_xml.contains(r#"<w:ind w:left="0" w:right="249""#),
        "12.472pt PDF footer right margin must round to a 249twip DOCX end indent: {footer_xml}"
    );
    assert!(
        footer_xml.contains(r#"<w:sz w:val="18""#) && footer_xml.contains("<w:i"),
        "9pt italic PDF footer must keep its text style: {footer_xml}"
    );
    let document_xml = read_document_xml(&output_docx);
    assert!(
        document_xml.contains(r#"<w:pgMar w:top="1800""#),
        "body top margin must be measured from the first body line after repeated chrome is removed: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"w:header="220" w:footer="200""#),
        "PDF chrome edge distances must become DOCX header/footer distances: {document_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_orders_two_column_text_by_column_flow() {
    let pdf_bytes = flate_pdf_with_page_contents(&[&[
        b"BT /F1 10 Tf 1 0 0 1 20 170 Tm (Left 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 140 Tm (Left 2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 170 Tm (Right 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 140 Tm (Right 2) Tj ET".as_slice(),
    ]
    .concat()]);
    let output_docx = temp_file_path("minimum_conversion_two_column_order", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("two-column-order.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Left 1".to_string(),
            "Left 2".to_string(),
            "Right 1".to_string(),
            "Right 2".to_string(),
        ]
    );
    let document_xml = read_document_xml(&output_docx);
    assert!(
        document_xml.contains(r#"<w:cols w:space="425" w:num="2" />"#),
        "detected two-column PDF text must use a real two-column DOCX section: {document_xml}"
    );
    assert_eq!(
        document_xml.matches(r#"<w:br w:type="column" />"#).count(),
        1,
        "two-column DOCX flow must contain exactly one explicit column break: {document_xml}"
    );
    let column_break_index = document_xml
        .find(r#"<w:br w:type="column" />"#)
        .expect("column break must exist");
    let right_one_index = document_xml
        .find(">Right 1</w:t>")
        .expect("right column's first line must exist");
    assert!(
        column_break_index < right_one_index
            && !document_xml[column_break_index..right_one_index].contains("</w:p>"),
        "column break must start the right column's first paragraph without adding a blank line: {document_xml}"
    );
    assert!(
        !document_xml.contains(r#"<w:ind w:left="4200""#),
        "right-column paragraphs must be positioned relative to their DOCX column instead of the full PDF page: {document_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// 1段見出しの後に続く2段本文だけを continuous section の実段組へ変換する。
#[test]
fn byte_conversion_keeps_full_width_heading_before_two_column_body() {
    let pdf_bytes = flate_pdf_with_page_contents(&[&[
        b"BT /F1 12 Tf 1 0 0 1 140 185 Tm (Heading) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Left 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 150 Tm (Right 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 120 Tm (Left 2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 120 Tm (Right 2) Tj ET".as_slice(),
    ]
    .concat()]);
    let output_docx = temp_file_path("minimum_conversion_heading_two_column_body", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("heading-columns.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Heading".to_string(),
            "Left 1".to_string(),
            "Left 2".to_string(),
            "Right 1".to_string(),
            "Right 2".to_string(),
        ]
    );
    let document_xml = read_document_xml(&output_docx);
    assert_eq!(
        document_xml.matches("<w:sectPr>").count(),
        2,
        "full-width heading and two-column body must use two DOCX sections: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"<w:cols w:space="425" w:num="1" />"#),
        "heading section must remain single-column: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"<w:cols w:space="425" w:num="2" /><w:type w:val="continuous" />"#),
        "body section must switch continuously into two real DOCX columns: {document_xml}"
    );
    assert_eq!(
        document_xml.matches(r#"<w:br w:type="column" />"#).count(),
        1,
        "two-column body must contain one explicit column break: {document_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// 2段本文の後にある1段注記を全幅の continuous section へ戻す。
#[test]
fn byte_conversion_restores_full_width_note_after_two_column_body() {
    let pdf_bytes = flate_pdf_with_page_contents(&[&[
        b"BT /F1 12 Tf 1 0 0 1 140 185 Tm (Heading) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Left 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 150 Tm (Right 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 120 Tm (Left 2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 120 Tm (Right 2) Tj ET".as_slice(),
        b"BT /F1 9 Tf 1 0 0 1 110 80 Tm (Full width note) Tj ET".as_slice(),
    ]
    .concat()]);
    let output_docx = temp_file_path("minimum_conversion_trailing_full_width_note", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("trailing-note.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Heading".to_string(),
            "Left 1".to_string(),
            "Left 2".to_string(),
            "Right 1".to_string(),
            "Right 2".to_string(),
            "Full width note".to_string(),
        ]
    );
    let document_xml = read_document_xml(&output_docx);
    assert_eq!(
        document_xml.matches("<w:sectPr>").count(),
        3,
        "heading, two-column body, and trailing note must use three sections: {document_xml}"
    );
    assert_eq!(
        document_xml
            .matches(r#"<w:cols w:space="425" w:num="1" />"#)
            .count(),
        2,
        "heading and trailing note must each use a single-column section: {document_xml}"
    );
    assert_eq!(
        document_xml
            .matches(r#"<w:cols w:space="425" w:num="2" />"#)
            .count(),
        1,
        "body must use exactly one two-column section: {document_xml}"
    );
    assert_eq!(
        document_xml
            .matches(r#"<w:type w:val="continuous" />"#)
            .count(),
        2,
        "both section transitions must remain on the PDF page: {document_xml}"
    );
    assert_eq!(
        document_xml.matches(r#"<w:br w:type="column" />"#).count(),
        1,
        "two-column body must contain one explicit column break: {document_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_preserves_page_breaks_between_structured_text_pages() {
    let pdf_bytes = flate_pdf_with_page_contents(&[
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (PageOne) Tj ET",
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (PageTwo) Tj ET",
    ]);
    let output_docx = temp_file_path("minimum_conversion_page_breaks", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("page-breaks.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec!["PageOne".to_string(), "PageTwo".to_string()]
    );
    assert_eq!(count_page_breaks_in_docx(&output_docx), 1);

    let _ = fs::remove_file(output_docx);
}

/// 複数ページごとの1段・2段構成を独立した nextPage section として復元する。
#[test]
fn byte_conversion_restores_columns_independently_across_pages() {
    let page_two = [
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Left 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 150 Tm (Right 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 120 Tm (Left 2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 120 Tm (Right 2) Tj ET".as_slice(),
    ]
    .concat();
    let pdf_bytes = flate_pdf_with_page_contents(&[
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Single page one) Tj ET",
        &page_two,
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Single page three) Tj ET",
    ]);
    let output_docx = temp_file_path("minimum_conversion_multipage_columns", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("multipage-columns.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Single page one".to_string(),
            "Left 1".to_string(),
            "Left 2".to_string(),
            "Right 1".to_string(),
            "Right 2".to_string(),
            "Single page three".to_string(),
        ]
    );
    let document_xml = read_document_xml(&output_docx);
    assert_eq!(
        document_xml.matches("<w:sectPr>").count(),
        3,
        "each PDF page must use an independent DOCX section: {document_xml}"
    );
    assert_eq!(
        document_xml
            .matches(r#"<w:cols w:space="425" w:num="1" />"#)
            .count(),
        2,
        "the first and third pages must remain single-column: {document_xml}"
    );
    assert_eq!(
        document_xml
            .matches(r#"<w:cols w:space="425" w:num="2" />"#)
            .count(),
        1,
        "only the middle page must use two real DOCX columns: {document_xml}"
    );
    assert_eq!(
        document_xml
            .matches(r#"<w:type w:val="nextPage" />"#)
            .count(),
        2,
        "the second and third PDF pages must start in new-page sections: {document_xml}"
    );
    assert_eq!(
        document_xml.matches(r#"<w:br w:type="column" />"#).count(),
        1,
        "the middle page must contain one explicit column break: {document_xml}"
    );
    assert_eq!(
        count_page_breaks_in_docx(&output_docx),
        0,
        "section boundaries must replace ad-hoc page-break runs"
    );

    let _ = fs::remove_file(output_docx);
}

/// 反復 header/footer を保持したまま、ページごとの段組み section を復元する。
#[test]
fn byte_conversion_restores_multipage_columns_with_repeated_chrome() {
    let page_one = [
        b"BT /F1 9 Tf 1 0 0 1 20 190 Tm (Shared Header) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 120 Tm (Single page one) Tj ET".as_slice(),
        b"BT /F1 9 Tf 1 0 0 1 20 10 Tm (Shared Footer) Tj ET".as_slice(),
    ]
    .concat();
    let page_two = [
        b"BT /F1 9 Tf 1 0 0 1 20 190 Tm (Shared Header) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Left 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 150 Tm (Right 1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 120 Tm (Left 2) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 120 Tm (Right 2) Tj ET".as_slice(),
        b"BT /F1 9 Tf 1 0 0 1 20 10 Tm (Shared Footer) Tj ET".as_slice(),
    ]
    .concat();
    let page_three = [
        b"BT /F1 9 Tf 1 0 0 1 20 190 Tm (Shared Header) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 120 Tm (Single page three) Tj ET".as_slice(),
        b"BT /F1 9 Tf 1 0 0 1 20 10 Tm (Shared Footer) Tj ET".as_slice(),
    ]
    .concat();
    let pdf_bytes = flate_pdf_with_page_contents(&[&page_one, &page_two, &page_three]);
    let output_docx = temp_file_path("minimum_conversion_multipage_columns_chrome", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("multipage-columns-chrome.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Single page one".to_string(),
            "Left 1".to_string(),
            "Left 2".to_string(),
            "Right 1".to_string(),
            "Right 2".to_string(),
            "Single page three".to_string(),
        ],
        "repeated page chrome must not remain in the document body"
    );
    let document_xml = read_document_xml(&output_docx);
    assert_eq!(
        document_xml.matches("<w:sectPr>").count(),
        3,
        "each PDF page must keep an independent DOCX section: {document_xml}"
    );
    assert_eq!(
        document_xml
            .matches(r#"<w:cols w:space="425" w:num="2" />"#)
            .count(),
        1,
        "the middle page must keep its real two-column section: {document_xml}"
    );
    assert_eq!(
        document_xml
            .matches(r#"<w:type w:val="nextPage" />"#)
            .count(),
        2,
        "page transitions must remain nextPage sections: {document_xml}"
    );
    assert_eq!(
        document_xml.matches("<w:headerReference").count(),
        3,
        "every section must reference the repeated PDF header: {document_xml}"
    );
    assert_eq!(
        document_xml.matches("<w:footerReference").count(),
        3,
        "every section must reference the repeated PDF footer: {document_xml}"
    );
    assert!(read_docx_entry(&output_docx, "word/header1.xml").contains("Shared Header"));
    assert!(read_docx_entry(&output_docx, "word/footer1.xml").contains("Shared Footer"));
    assert_eq!(
        document_xml.matches(r#"<w:br w:type="column" />"#).count(),
        1,
        "the middle page must contain one explicit column break: {document_xml}"
    );
    assert_eq!(
        count_page_breaks_in_docx(&output_docx),
        0,
        "section boundaries must replace ad-hoc page-break runs"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_inserts_table_at_pdf_position_and_suppresses_duplicate_cell_paragraphs() {
    let content = [
        b"BT /F1 10 Tf 1 0 0 1 20 180 Tm (Intro) Tj ET".as_slice(),
        b"20 150 m 120 150 l S 20 100 m 120 100 l S 20 100 m 20 150 l S 70 100 m 70 150 l S 120 100 m 120 150 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 125 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 125 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 50 Tm (Outro) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_table_position", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("table-position.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Intro".to_string(),
            "A1".to_string(),
            "B1".to_string(),
            "Outro".to_string(),
        ]
    );
    let document_xml = read_document_xml(&output_docx);
    assert!(
        document_xml.contains(r#"<w:spacing w:before="0" w:after="600""#),
        "30pt PDF gap from intro text to table must become 600twips paragraph after-spacing: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"<w:spacing w:before="800" w:after="0""#),
        "40pt PDF gap from table bottom to outro text must become 800twips paragraph before-spacing: {document_xml}"
    );

    assert!(
        document_xml.contains(r#"<w:vAlign w:val="center""#),
        "positioned PDF table cell text must remain vertically centered in DOCX: {document_xml}"
    );
    assert!(
        document_xml.contains("<w:cantSplit"),
        "a PDF table row must not split across DOCX pages: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"<w:spacing w:before="0" w:after="0""#),
        "positioned PDF table cell paragraphs must not inherit renderer-dependent spacing: {document_xml}"
    );
    assert!(
        document_xml.contains(
            r#"<w:tblCellMar><w:top w:w="0" w:type="dxa" /><w:left w:w="108" w:type="dxa" /><w:bottom w:w="0" w:type="dxa" /><w:right w:w="108" w:type="dxa" /></w:tblCellMar>"#
        ),
        "positioned PDF table cells must use explicit stable padding: {document_xml}"
    );
    assert!(
        document_xml.contains(
            r#"<w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:cs="Arial" w:eastAsia="Arial" />"#
        ),
        "positioned PDF table cells must preserve a usable PDF font family: {document_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_preserves_multiple_tables_on_same_page() {
    let content = [
        b"BT /F1 10 Tf 1 0 0 1 20 180 Tm (Intro) Tj ET".as_slice(),
        b"20 150 m 120 150 l S 20 120 m 120 120 l S 20 120 m 20 150 l S 70 120 m 70 150 l S 120 120 m 120 150 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 132 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 132 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 100 Tm (Middle) Tj ET".as_slice(),
        b"20 80 m 120 80 l S 20 50 m 120 50 l S 20 50 m 20 80 l S 70 50 m 70 80 l S 120 50 m 120 80 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 62 Tm (C1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 62 Tm (D1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 30 Tm (Outro) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_multi_table_position", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("multi-table.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 2);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Intro".to_string(),
            "A1".to_string(),
            "B1".to_string(),
            "Middle".to_string(),
            "C1".to_string(),
            "D1".to_string(),
            "Outro".to_string(),
        ]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_keeps_visual_flow_when_right_margin_labels_and_tables_coexist() {
    let content = [
        b"BT /F1 10 Tf 1 0 0 1 20 180 Tm (Heading one) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 180 Tm (Example one) Tj ET".as_slice(),
        b"20 150 m 120 150 l S 20 120 m 120 120 l S 20 120 m 20 150 l S 70 120 m 70 150 l S 120 120 m 120 150 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 132 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 132 Tm (B1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 100 Tm (Heading two) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 230 100 Tm (Example two) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_right_labels_table_flow", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("right-labels-table-flow.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Heading one".to_string(),
            "Example one".to_string(),
            "A1".to_string(),
            "B1".to_string(),
            "Heading two".to_string(),
            "Example two".to_string(),
        ]
    );
    let document_xml = read_document_xml(&output_docx);
    assert!(
        document_xml.contains(r#"<w:cols w:space="425" w:num="1" />"#),
        "table-mixed layouts must remain a single-column DOCX section: {document_xml}"
    );
    assert!(
        !document_xml.contains(r#"<w:br w:type="column" />"#),
        "table-mixed layouts must not receive a false column break: {document_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// PDF の文字幅から求めた余白と表幅を DOCX の段落・表へ保持する。
#[test]
fn byte_conversion_preserves_pdf_alignment_and_table_width_in_docx_properties() {
    let content = [
        b"BT /F1 10 Tf 1 0 0 1 128 180 Tm (Centered) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 260 160 Tm (R) Tj ET".as_slice(),
        b"20 130 m 220 130 l S 20 100 m 220 100 l S 20 100 m 20 130 l S 120 100 m 120 130 l S 220 100 m 220 130 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 112 Tm (Before) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 130 112 Tm (After) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_pdf_alignment_table_width", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("alignment-width.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:jc w:val="center""#),
        "centered PDF text must become a centered DOCX paragraph: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:jc w:val="right""#),
        "right PDF text must become a right-aligned DOCX paragraph: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:ind w:left="0" w:right="400""#),
        "centered PDF text must compensate the 20pt section left margin with a symmetric 400twip right indent: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:ind w:left="0" w:right="656""#),
        "32.78pt PDF right margin must round to a 656twip right paragraph indent: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:spacing w:before="200" w:after="600""#),
        "30pt PDF gap before the following table must become 600twips paragraph after-spacing: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:tblW w:w="4000" w:type="dxa""#),
        "200pt PDF table width must become 4000twips DOCX table width: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:tblLayout w:type="fixed""#),
        "positioned PDF table widths must not be changed by DOCX autofit: {doc_xml}"
    );
    assert!(
        doc_xml.contains(
            r#"<w:tblGrid><w:gridCol w:w="2000" w:type="dxa" /><w:gridCol w:w="2000" w:type="dxa" /></w:tblGrid>"#
        ),
        "two 100pt PDF columns must become an explicit 2000twip DOCX table grid: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:trHeight w:val="600" w:hRule="exact""#),
        "30pt PDF table row height must become exact 600twips DOCX row height: {doc_xml}"
    );
    assert!(
        !doc_xml.contains("<w:tblInd"),
        "left-most PDF table x offset is the page layout origin and must not be double-counted as DOCX indent: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_normalizes_pdf_page_margin_before_docx_indents() {
    let content = [
        b"BT /F1 10 Tf 1 0 0 1 170 180 Tm (Heading) Tj ET".as_slice(),
        b"147 130 m 547 130 l S 147 100 m 547 100 l S 147 100 m 147 130 l S 347 100 m 347 130 l S 547 100 m 547 130 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 157 112 Tm (Before) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 357 112 Tm (After) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 170 80 Tm (Outro) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_pdf_margin_normalized", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("margin-normalized.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:ind w:left="460""#),
        "23pt relative paragraph offset must become 460twips after subtracting the PDF page margin: {doc_xml}"
    );
    assert!(
        !doc_xml.contains(r#"<w:ind w:left="3400""#),
        "170pt absolute PDF x offset must not be used directly as DOCX paragraph indent: {doc_xml}"
    );
    assert!(
        !doc_xml.contains("<w:tblInd"),
        "147pt PDF page margin must not become a DOCX table indent: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// PDF の MediaBox 寸法を DOCX のページサイズとして保持する。
#[test]
fn byte_conversion_preserves_pdf_page_size_in_docx() {
    let pdf_bytes =
        flate_pdf_with_page_contents(&[b"BT /F1 10 Tf 1 0 0 1 20 150 Tm (Page size) Tj ET"]);
    let output_docx = temp_file_path("minimum_conversion_pdf_page_size", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("page-size.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="6000" w:h="4000" w:orient="landscape" />"#),
        "300x200pt PDF page must become a 6000x4000twip DOCX page: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// MediaBox と異なる CropBox の表示寸法と原点を DOCX layout へ反映する。
#[test]
fn byte_conversion_uses_pdf_crop_box_for_page_size_and_content_origin() {
    let pdf_bytes = flate_pdf_with_page_content_and_crop_box(
        b"BT /F1 10 Tf 1 0 0 1 70 125 Tm (Cropped page) Tj ET",
        (300, 200),
        (50, 25, 250, 175),
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_crop_box", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("crop-box.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="4000" w:h="3000" w:orient="landscape" />"#),
        "200x150pt CropBox must determine the DOCX display page size: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:pgMar w:top="800" w:right="0" w:bottom="0" w:left="400""#),
        "content at (70,125) must be positioned relative to CropBox origin (50,25): {doc_xml}"
    );
    assert!(
        !doc_xml.contains(r#"w:left="1400""#),
        "absolute MediaBox x coordinates must not leak into DOCX margins: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// 親Pages nodeから継承したCropBoxもDOCX layoutへ反映する。
#[test]
fn byte_conversion_uses_inherited_pdf_crop_box_for_page_layout() {
    let pdf_bytes = valid_pdf_with_inherited_page_boxes(
        b"BT /F1 10 Tf 1 0 0 1 70 125 Tm (Inherited crop) Tj ET",
        (300, 200),
        (50, 25, 250, 175),
    );
    let output_docx = temp_file_path("minimum_conversion_inherited_pdf_crop_box", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("inherited-crop-box.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="4000" w:h="3000" w:orient="landscape" />"#),
        "inherited 200x150pt CropBox must determine DOCX page size: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:pgMar w:top="800" w:right="0" w:bottom="0" w:left="400""#),
        "content must be positioned relative to the inherited CropBox origin: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// native path変換でも親Pages nodeのCropBox継承を維持する。
#[test]
fn path_conversion_uses_inherited_pdf_crop_box_for_page_layout() {
    let input_pdf = temp_file_path("minimum_conversion_inherited_crop_box_path", "pdf");
    let output_docx = temp_file_path("minimum_conversion_inherited_crop_box_path", "docx");
    let pdf_bytes = valid_pdf_with_inherited_page_boxes(
        b"BT /F1 10 Tf 1 0 0 1 70 125 Tm (Inherited crop) Tj ET",
        (300, 200),
        (50, 25, 250, 175),
    );
    fs::write(&input_pdf, pdf_bytes).expect("failed to write inherited CropBox PDF");

    convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
        input_pdf_path: input_pdf.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    })
    .expect("path conversion must succeed");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="4000" w:h="3000" w:orient="landscape" />"#),
        "native path conversion must use the inherited 200x150pt CropBox: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:pgMar w:top="800" w:right="0" w:bottom="0" w:left="400""#),
        "native path content must be relative to inherited CropBox origin: {doc_xml}"
    );

    let _ = fs::remove_file(input_pdf);
    let _ = fs::remove_file(output_docx);
}

/// 親Pages nodeから継承したRotateをCropBox表示寸法と本文座標へ反映する。
#[test]
fn byte_conversion_applies_inherited_rotation_to_cropped_page_layout() {
    let pdf_bytes = valid_pdf_with_inherited_page_boxes_and_rotation(
        b"q 0 1 -1 0 90 45 cm BT /F1 10 Tf 1 0 0 1 0 0 Tm (Inherited rotate) Tj ET Q",
        (300, 200),
        (50, 25, 250, 175),
        90,
    );
    let output_docx = temp_file_path("minimum_conversion_inherited_pdf_rotation", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("inherited-crop-box-rotation.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="3000" w:h="4000" />"#),
        "90-degree rotation must turn the inherited 200x150pt CropBox into a 150x200pt DOCX page: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:pgMar w:top="620" w:right="0" w:bottom="0" w:left="400""#),
        "rotated Helvetica text must include the DOCX renderer's 1pt top-bearing compensation: {doc_xml}"
    );
    assert!(
        !doc_xml.contains(r#"<w:jc w:val="center""#),
        "left-positioned rotated text must not be centered by an overestimated text width: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// native path変換でも継承RotateとCropBoxを同じ表示座標へ合成する。
#[test]
fn path_conversion_applies_inherited_rotation_to_cropped_page_layout() {
    let input_pdf = temp_file_path("minimum_conversion_inherited_rotation_path", "pdf");
    let output_docx = temp_file_path("minimum_conversion_inherited_rotation_path", "docx");
    let pdf_bytes = valid_pdf_with_inherited_page_boxes_and_rotation(
        b"q 0 1 -1 0 90 45 cm BT /F1 10 Tf 1 0 0 1 0 0 Tm (Inherited rotate) Tj ET Q",
        (300, 200),
        (50, 25, 250, 175),
        90,
    );
    fs::write(&input_pdf, pdf_bytes).expect("failed to write inherited Rotate PDF");
    let parsed = parse_pdf(&input_pdf).expect("native parser must read inherited Rotate PDF");
    assert_eq!(
        parsed.pages,
        vec![ParsedPage {
            page_number: 0,
            width: 200.0,
            height: 150.0,
            rotation: 90,
        }],
        "native parser must preserve unrotated CropBox dimensions with inherited rotation"
    );
    let text_bounds = parsed.text_elements[0].bounds;
    assert!(
        (text_bounds.x0 - 20.0).abs() < 0.01
            && (text_bounds.y0 - 160.0).abs() < 0.01
            && (text_bounds.x1 - 87.26).abs() < 0.01
            && (text_bounds.y1 - 170.0).abs() < 0.01,
        "text bounds must compose the counter-rotated content matrix with Helvetica glyph widths: {text_bounds:?}"
    );

    convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
        input_pdf_path: input_pdf.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    })
    .expect("path conversion must succeed");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="3000" w:h="4000" />"#),
        "native path must use the 150x200pt rotated CropBox: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:pgMar w:top="620" w:right="0" w:bottom="0" w:left="400""#),
        "native path must compensate the rotated Helvetica top bearing: {doc_xml}"
    );
    assert!(
        !doc_xml.contains(r#"<w:jc w:val="center""#),
        "native path must keep the rotated text left-positioned: {doc_xml}"
    );

    let _ = fs::remove_file(input_pdf);
    let _ = fs::remove_file(output_docx);
}

/// native path変換でもCropBox由来の表示page layoutを維持する。
#[test]
fn path_conversion_uses_pdf_crop_box_for_page_size_and_content_origin() {
    let input_pdf = temp_file_path("minimum_conversion_pdf_crop_box_path", "pdf");
    let output_docx = temp_file_path("minimum_conversion_pdf_crop_box_path", "docx");
    let pdf_bytes = valid_pdf_with_page_content_and_crop_box(
        b"BT /F1 10 Tf 1 0 0 1 70 125 Tm (Cropped page) Tj ET",
        (300, 200),
        (50, 25, 250, 175),
    );
    fs::write(&input_pdf, pdf_bytes).expect("failed to write CropBox PDF");

    convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
        input_pdf_path: input_pdf.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    })
    .expect("path conversion must succeed");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="4000" w:h="3000" w:orient="landscape" />"#),
        "native path conversion must use the 200x150pt CropBox page size: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:pgMar w:top="800" w:right="0" w:bottom="0" w:left="400""#),
        "native path content must remain relative to CropBox origin: {doc_xml}"
    );

    let _ = fs::remove_file(input_pdf);
    let _ = fs::remove_file(output_docx);
}

/// ページごとに異なる MediaBox 寸法を個別の DOCX section として保持する。
#[test]
fn byte_conversion_preserves_mixed_pdf_page_sizes_in_docx_sections() {
    let pdf_bytes = flate_pdf_with_page_contents_base_fonts_and_page_sizes(
        &[
            b"BT /F1 8 Tf 1 0 0 1 20 185 Tm (Shared Header) Tj ET BT /F1 10 Tf 1 0 0 1 20 150 Tm (Landscape) Tj ET",
            b"BT /F1 8 Tf 1 0 0 1 20 285 Tm (Shared Header) Tj ET BT /F1 10 Tf 1 0 0 1 20 250 Tm (Portrait) Tj ET",
        ],
        &[("F1", "Helvetica")],
        &[(300, 200), (200, 300)],
    );
    let output_docx = temp_file_path("minimum_conversion_mixed_pdf_page_sizes", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("mixed-page-size.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="6000" w:h="4000" w:orient="landscape" />"#),
        "first PDF page size must be preserved in a DOCX section: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="4000" w:h="6000" />"#),
        "second PDF page size must be preserved in a DOCX section: {doc_xml}"
    );
    assert!(
        doc_xml.matches("<w:sectPr>").count() >= 2,
        "mixed page sizes must create at least two DOCX sections: {doc_xml}"
    );
    assert_eq!(
        doc_xml.matches("<w:headerReference").count(),
        2,
        "repeated PDF headers must remain linked from every mixed-size section: {doc_xml}"
    );
    assert!(read_docx_entry(&output_docx, "word/header1.xml").contains("Shared Header"));
    assert!(read_docx_entry(&output_docx, "word/header2.xml").contains("Shared Header"));

    let _ = fs::remove_file(output_docx);
}

/// PDF の直角 page rotation を DOCX の表示ページ寸法と向きへ反映する。
#[test]
fn byte_conversion_applies_right_angle_pdf_page_rotations_to_docx_page_size() {
    for rotation in [90, 270, -90] {
        let pdf_bytes = flate_pdf_with_page_contents_base_fonts_page_sizes_and_rotations(
            &[b"BT /F1 10 Tf 1 0 0 1 20 250 Tm (Rotated page) Tj ET"],
            &[("F1", "Helvetica")],
            &[(200, 300)],
            &[rotation],
        );
        let output_docx = temp_file_path(
            format!("minimum_conversion_rotated_pdf_page_{rotation}").as_str(),
            "docx",
        );

        let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("rotated-page.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
        fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

        let doc_xml = read_document_xml(&output_docx);
        assert!(
            doc_xml.contains(r#"<w:pgSz w:w="6000" w:h="4000" w:orient="landscape" />"#),
            "200x300pt PDF rotated {rotation} degrees must become a 6000x4000twip landscape DOCX page: {doc_xml}"
        );

        let _ = fs::remove_file(output_docx);
    }
}

#[test]
fn byte_conversion_preserves_pdf_text_run_style_in_docx_properties() {
    let pdf_bytes = flate_pdf_with_page_contents_and_base_font(
        &[b"BT 1 0 0 rg /F1 16 Tf 1 0 0 1 20 150 Tm (Styled) Tj ET"],
        "Helvetica-BoldOblique",
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_text_run_style", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("text-style.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:sz w:val="32""#),
        "16pt PDF font size must become 32 half-points DOCX run size: {doc_xml}"
    );
    assert!(
        doc_xml.contains("<w:b"),
        "bold PDF BaseFont must become DOCX bold run property: {doc_xml}"
    );
    assert!(
        doc_xml.contains("<w:i"),
        "oblique PDF BaseFont must become DOCX italic run property: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:color w:val="FF0000""#),
        "red PDF fill color must become DOCX run color: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:rFonts w:ascii="Arial""#),
        "PDF Helvetica BaseFont must use a renderer-resolvable Arial family: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_preserves_mixed_pdf_text_run_styles_on_same_line() {
    let content = [
        b"BT 1 0 0 rg /F1 14 Tf 1 0 0 1 20 150 Tm (Red) Tj ET".as_slice(),
        b"BT 0 0 1 rg /F2 10 Tf 1 0 0 1 48 150 Tm (Blue) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents_and_base_fonts(
        &[&content],
        &[("F1", "Helvetica-Bold"), ("F2", "Times-Italic")],
    );
    let output_docx = temp_file_path("minimum_conversion_pdf_mixed_text_run_style", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("mixed-text-style.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(">Red</w:t>") && doc_xml.contains(">Blue</w:t>"),
        "same-line text fragments must remain in document order: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:color w:val="FF0000""#)
            && doc_xml.contains(r#"<w:color w:val="0000FF""#),
        "same-line PDF text fragments must keep independent fill colors: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:sz w:val="28""#) && doc_xml.contains(r#"<w:sz w:val="20""#),
        "same-line PDF text fragments must keep independent font sizes: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:rFonts w:ascii="Arial""#)
            && doc_xml.contains(r#"<w:rFonts w:ascii="Times New Roman""#),
        "same-line PDF text fragments must keep independently normalized font families: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_converts_short_pdf_rule_below_text_to_run_underline() {
    let content = [
        b"BT /F1 16 Tf 1 0 0 1 20 150 Tm (Underlined) Tj ET".as_slice(),
        b"20 147 m 96 147 l S".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_pdf_text_underline", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("underlined-text.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(">Underlined</w:t>"),
        "underlined PDF text must remain as text: {doc_xml}"
    );
    assert!(
        doc_xml.contains(r#"<w:u w:val="single""#),
        "short PDF rule directly below text must become DOCX run underline: {doc_xml}"
    );
    assert!(
        count_table_tags_in_docx(&output_docx) == 0,
        "text underline must not be inferred as a table: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_converts_short_pdf_rule_below_table_cell_text_to_underline() {
    let content = [
        b"0 100 m 200 100 l S 0 0 m 200 0 l S 0 0 m 0 100 l S 100 0 m 100 100 l S 200 0 m 200 100 l S".as_slice(),
        b"BT /F1 16 Tf 1 0 0 1 10 50 Tm (Left) Tj ET".as_slice(),
        b"10 47 m 46 47 l S".as_slice(),
        b"BT /F1 16 Tf 1 0 0 1 110 50 Tm (Right) Tj ET".as_slice(),
        b"110 47 m 154 47 l S".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_pdf_table_cell_underline", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("underlined-cell-text.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(">Left</w:t>") && doc_xml.contains(">Right</w:t>"),
        "table cell text must remain in cells: {doc_xml}"
    );
    assert!(
        doc_xml.matches(r#"<w:u w:val="single""#).count() >= 2,
        "short PDF rules below text in table cells must become DOCX run underline: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_preserves_pdf_cmyk_text_fill_color_in_docx_properties() {
    let pdf_bytes =
        flate_pdf_with_page_contents(&[b"BT 1 0 0 0 k /F1 12 Tf 1 0 0 1 20 150 Tm (Cyan) Tj ET"]);
    let output_docx = temp_file_path("minimum_conversion_pdf_cmyk_text_color", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("cmyk-text-color.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:color w:val="00FFFF""#),
        "PDF DeviceCMYK cyan fill color must become DOCX RGB run color: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_keeps_single_column_graphic_box_as_paragraph_flow() {
    let content = [
        b"BT /F1 10 Tf 1 0 0 1 30 165 Tm (Sidebar) Tj ET".as_slice(),
        b"20 180 m 70 180 l S 20 150 m 70 150 l S 20 150 m 20 180 l S 70 150 m 70 180 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 20 125 Tm (Before table) Tj ET".as_slice(),
        b"20 110 m 120 110 l S 20 80 m 120 80 l S 20 80 m 20 110 l S 70 80 m 70 110 l S 120 80 m 120 110 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 92 Tm (A1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 80 92 Tm (B1) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_single_column_box", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("single-column-box.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(count_table_tags_in_docx(&output_docx), 1);
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Sidebar".to_string(),
            "Before table".to_string(),
            "A1".to_string(),
            "B1".to_string(),
        ]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_preserves_wide_single_column_rule_boxes_as_tables() {
    let content = [
        b"BT /F1 10 Tf 1 0 0 1 20 185 Tm (Intro) Tj ET".as_slice(),
        b"20 160 m 260 160 l S 20 125 m 260 125 l S 20 125 m 20 160 l S 260 125 m 260 160 l S"
            .as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 142 Tm (Amendment one) Tj ET".as_slice(),
        b"20 95 m 260 95 l S 20 60 m 260 60 l S 20 60 m 20 95 l S 260 60 m 260 95 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 30 77 Tm (Amendment two) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_wide_single_column_boxes", "docx");

    let docx_bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("wide-single-column-boxes.pdf", &pdf_bytes)
            .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    assert_eq!(
        count_table_tags_in_docx(&output_docx),
        2,
        "wide one-column rule boxes must stay as bordered DOCX tables"
    );
    let doc_xml = read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"</w:tbl><w:p w14:paraId=""#)
            && doc_xml.contains(r#"<w:spacing w:after="160" />"#),
        "consecutive tables must have a small paragraph gap between them: {doc_xml}"
    );
    assert_eq!(
        doc_xml.matches(r#"<w:spacing w:after="160" />"#).count(),
        1,
        "only the gap between consecutive tables should add spacing: {doc_xml}"
    );
    assert_eq!(
        text_nodes_in_docx(&output_docx),
        vec![
            "Intro".to_string(),
            "Amendment one".to_string(),
            "Amendment two".to_string(),
        ]
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn inferred_table_assigns_text_elements_to_cells() {
    let parsed = ParsedDocument {
        quality: Default::default(),
        path: "synthetic.pdf".into(),
        byte_len: 0,
        header: "%PDF-1.4".to_string(),
        version: PdfVersion { major: 1, minor: 4 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 200.0,
            height: 200.0,
            rotation: 0,
        }],
        graphics: vec![
            horizontal(0.0),
            horizontal(50.0),
            horizontal(100.0),
            vertical(0.0),
            vertical(80.0),
            vertical(160.0),
        ],
        text_elements: vec![
            text_element(10.0, 70.0, "A1"),
            text_element(90.0, 70.0, "B1"),
            text_element(10.0, 20.0, "A2"),
            text_element(90.0, 20.0, "B2"),
        ],
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: "A1\nB1\nA2\nB2".to_string(),
    };

    let table =
        jade_pdf2docx_docx::infer_ocr_table_from_pdf_graphics(&parsed).expect("table inferred");
    assert_eq!(
        table.rows,
        vec![
            vec!["A1".to_string(), "B1".to_string()],
            vec!["A2".to_string(), "B2".to_string()],
        ]
    );
}

#[test]
fn inferred_table_exposes_spans_from_missing_graphic_rules() {
    let parsed = ParsedDocument {
        quality: Default::default(),
        path: "synthetic.pdf".into(),
        byte_len: 0,
        header: "%PDF-1.4".to_string(),
        version: PdfVersion { major: 1, minor: 4 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 200.0,
            height: 200.0,
            rotation: 0,
        }],
        graphics: vec![
            horizontal(0.0),
            horizontal(50.0),
            horizontal(100.0),
            vertical(0.0),
            ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 80.0,
                    y0: 0.0,
                    x1: 80.0,
                    y1: 50.0,
                },
                stroke_width: 0.5,
            },
            vertical(160.0),
        ],
        text_elements: vec![
            text_element(10.0, 70.0, "A"),
            text_element(10.0, 20.0, "C"),
            text_element(90.0, 20.0, "D"),
        ],
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: "A\nB\nC\nD".to_string(),
    };

    let table =
        jade_pdf2docx_docx::infer_ocr_table_from_pdf_graphics(&parsed).expect("table inferred");

    table
        .validate()
        .expect("inferred table spans must validate");
    assert_eq!(table.spans[0][0], OcrCellSpan::new(1, 2));
    assert_eq!(table.spans[0][1], OcrCellSpan::single());
}

#[test]
fn inferred_table_keeps_non_empty_cells_split_when_middle_rule_is_missing() {
    let parsed = ParsedDocument {
        quality: Default::default(),
        path: "synthetic.pdf".into(),
        byte_len: 0,
        header: "%PDF-1.4".to_string(),
        version: PdfVersion { major: 1, minor: 4 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 200.0,
            height: 200.0,
            rotation: 0,
        }],
        graphics: vec![
            horizontal(0.0),
            horizontal(50.0),
            horizontal(100.0),
            vertical(0.0),
            ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 80.0,
                    y0: 0.0,
                    x1: 80.0,
                    y1: 50.0,
                },
                stroke_width: 0.5,
            },
            vertical(160.0),
        ],
        text_elements: vec![
            text_element(10.0, 70.0, "Left body"),
            text_element(90.0, 70.0, "Right body"),
            text_element(10.0, 20.0, "C"),
            text_element(90.0, 20.0, "D"),
        ],
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: "Left body\nRight body\nC\nD".to_string(),
    };

    let table =
        jade_pdf2docx_docx::infer_ocr_table_from_pdf_graphics(&parsed).expect("table inferred");

    table
        .validate()
        .expect("inferred table spans must validate");
    assert_eq!(table.spans[0][0], OcrCellSpan::single());
    assert_eq!(table.spans[0][1], OcrCellSpan::single());
}

#[test]
fn inferred_table_keeps_cells_split_when_vertical_rule_stops_near_row_boundary() {
    let parsed = ParsedDocument {
        quality: Default::default(),
        path: "synthetic.pdf".into(),
        byte_len: 0,
        header: "%PDF-1.4".to_string(),
        version: PdfVersion { major: 1, minor: 4 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 200.0,
            height: 200.0,
            rotation: 0,
        }],
        graphics: vec![
            horizontal(0.0),
            horizontal(50.0),
            horizontal(100.0),
            vertical(0.0),
            ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 80.0,
                    y0: 0.0,
                    x1: 80.0,
                    y1: 48.0,
                },
                stroke_width: 0.5,
            },
            ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 80.0,
                    y0: 50.0,
                    x1: 80.0,
                    y1: 100.0,
                },
                stroke_width: 0.5,
            },
            vertical(160.0),
        ],
        text_elements: vec![
            text_element(10.0, 70.0, "A1"),
            text_element(90.0, 70.0, "B1"),
            text_element(10.0, 20.0, "A2"),
            text_element(90.0, 20.0, "B2"),
        ],
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: "A1\nB1\nA2\nB2".to_string(),
    };

    let table =
        jade_pdf2docx_docx::infer_ocr_table_from_pdf_graphics(&parsed).expect("table inferred");

    table
        .validate()
        .expect("inferred table spans must validate");
    assert_eq!(table.spans[0][0], OcrCellSpan::single());
    assert_eq!(table.spans[0][1], OcrCellSpan::single());
    assert_eq!(table.spans[1][0], OcrCellSpan::single());
    assert_eq!(table.spans[1][1], OcrCellSpan::single());
}

#[test]
fn byte_conversion_preserves_nested_tables_inside_outer_cells() {
    let content = [
        b"0 160 m 280 160 l S 0 80 m 280 80 l S 0 0 m 280 0 l S 0 0 m 0 160 l S 140 0 m 140 160 l S 280 0 m 280 160 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 52 130 Tm (Before) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 192 130 Tm (After) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 12 54 Tm (Left note) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 152 54 Tm (Right note) Tj ET".as_slice(),
        b"10 54 m 130 54 l S 10 32 m 130 32 l S 10 10 m 130 10 l S 10 10 m 10 54 l S 70 10 m 70 54 l S 130 10 m 130 54 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 22 40 Tm (L1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 82 40 Tm (L2) Tj ET".as_slice(),
        b"150 54 m 270 54 l S 150 32 m 270 32 l S 150 10 m 270 10 l S 150 10 m 150 54 l S 210 10 m 210 54 l S 270 10 m 270 54 l S".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 162 40 Tm (R1) Tj ET".as_slice(),
        b"BT /F1 10 Tf 1 0 0 1 222 40 Tm (R2) Tj ET".as_slice(),
    ]
    .join(&b' ');
    let pdf_bytes = flate_pdf_with_page_contents(&[&content]);
    let output_docx = temp_file_path("minimum_conversion_nested_tables", "docx");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("nested-tables.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = read_document_xml(&output_docx);
    assert_eq!(
        count_table_tags_in_docx(&output_docx),
        3,
        "outer table and two inner tables must all be emitted: {doc_xml}"
    );
    assert!(
        doc_xml.contains("<w:tc><w:tcPr")
            && doc_xml.contains(">Left note</w:t>")
            && doc_xml.contains(">L1</w:t>")
            && doc_xml.contains("</w:p><w:tbl>"),
        "left inner table must be nested inside the outer left cell, after its cell text: {doc_xml}"
    );
    assert!(
        doc_xml.contains(">Right note</w:t>") && doc_xml.contains(">R1</w:t>"),
        "right inner table text must be preserved inside the outer right cell: {doc_xml}"
    );
    assert!(
        !doc_xml.contains("</w:tbl></w:tc>"),
        "Word repairs DOCX cells that end immediately after a nested table; emit a trailing paragraph after nested tables: {doc_xml}"
    );

    let _ = fs::remove_file(output_docx);
}

/// 装飾線がある長文ページも、列位置の反復だけで巨大な表にしない。
#[test]
fn dense_article_with_a_decorative_line_is_not_a_text_grid_table() {
    let texts = (0..30)
        .flat_map(|row| {
            let y = 600.0 - row as f32 * 15.0;
            [
                text_element(10.0, y, "article"),
                text_element(150.0, y, "body"),
            ]
        })
        .collect();
    let parsed = ParsedDocument {
        quality: Default::default(),
        path: "article.pdf".into(),
        byte_len: 0,
        header: "%PDF-1.4".to_string(),
        version: PdfVersion { major: 1, minor: 4 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 300.0,
            height: 700.0,
            rotation: 0,
        }],
        graphics: vec![horizontal(650.0)],
        text_elements: texts,
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: "article body".to_string(),
    };
    assert!(jade_pdf2docx_docx::infer_ocr_tables_from_pdf_graphics(&parsed).is_empty());
}

#[test]
fn inferred_table_exposes_rowspans_from_missing_horizontal_rules() {
    let parsed = ParsedDocument {
        quality: Default::default(),
        path: "synthetic.pdf".into(),
        byte_len: 0,
        header: "%PDF-1.4".to_string(),
        version: PdfVersion { major: 1, minor: 4 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 200.0,
            height: 200.0,
            rotation: 0,
        }],
        graphics: vec![
            horizontal(0.0),
            ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 80.0,
                    y0: 50.0,
                    x1: 160.0,
                    y1: 50.0,
                },
                stroke_width: 0.5,
            },
            horizontal(100.0),
            vertical(0.0),
            vertical(80.0),
            vertical(160.0),
        ],
        text_elements: vec![
            text_element(10.0, 70.0, "A"),
            text_element(90.0, 70.0, "B"),
            text_element(10.0, 20.0, "C"),
            text_element(90.0, 20.0, "D"),
        ],
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: "A\nB\nC\nD".to_string(),
    };

    let table =
        jade_pdf2docx_docx::infer_ocr_table_from_pdf_graphics(&parsed).expect("table inferred");

    table
        .validate()
        .expect("inferred table spans must validate");
    assert_eq!(table.spans[0][0], OcrCellSpan::new(2, 1));
    assert_eq!(table.spans[1][0], OcrCellSpan::single());
}

#[test]
fn inferred_table_uses_one_page_grid_without_mixing_other_pages() {
    let mut page_1_graphics = vec![
        horizontal(0.0),
        horizontal(50.0),
        horizontal(100.0),
        vertical(0.0),
        vertical(80.0),
        vertical(160.0),
    ];
    for graphic in &mut page_1_graphics {
        graphic.page = 1;
    }
    let parsed = ParsedDocument {
        quality: Default::default(),
        path: "synthetic.pdf".into(),
        byte_len: 0,
        header: "%PDF-1.4".to_string(),
        version: PdfVersion { major: 1, minor: 4 },
        page_count: 2,
        pages: vec![
            ParsedPage {
                page_number: 0,
                width: 200.0,
                height: 200.0,
                rotation: 0,
            },
            ParsedPage {
                page_number: 1,
                width: 200.0,
                height: 200.0,
                rotation: 0,
            },
        ],
        graphics: page_1_graphics,
        text_elements: vec![
            text_element(10.0, 70.0, "wrong-page"),
            text_element_on_page(1, 10.0, 70.0, "A1"),
            text_element_on_page(1, 90.0, 20.0, "B2"),
        ],
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: "wrong-page\nA1\nB2".to_string(),
    };

    let table =
        jade_pdf2docx_docx::infer_ocr_table_from_pdf_graphics(&parsed).expect("table inferred");

    assert_eq!(table.rows[0][0], "A1");
    assert_eq!(table.rows[1][1], "B2");
    assert!(
        table
            .rows
            .iter()
            .flatten()
            .all(|cell| !cell.contains("wrong-page"))
    );
}

#[test]
fn inferred_table_falls_back_to_repeated_text_columns_when_grid_lines_are_missing() {
    let parsed = ParsedDocument {
        quality: Default::default(),
        path: "synthetic.pdf".into(),
        byte_len: 0,
        header: "%PDF-1.4".to_string(),
        version: PdfVersion { major: 1, minor: 4 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 300.0,
            height: 200.0,
            rotation: 0,
        }],
        graphics: Vec::new(),
        text_elements: vec![
            text_element(20.0, 150.0, "A"),
            text_element(34.0, 150.0, "1"),
            text_element(160.0, 150.0, "B1"),
            text_element(20.0, 120.0, "A2"),
            text_element(160.0, 120.0, "B2"),
            text_element(20.0, 90.0, "A3"),
            text_element(160.0, 90.0, "B3"),
        ],
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: "A1\nB1\nA2\nB2\nA3\nB3".to_string(),
    };

    let table =
        jade_pdf2docx_docx::infer_ocr_table_from_pdf_graphics(&parsed).expect("table inferred");

    assert_eq!(
        table.rows,
        vec![
            vec!["A 1".to_string(), "B1".to_string()],
            vec!["A2".to_string(), "B2".to_string()],
            vec!["A3".to_string(), "B3".to_string()],
        ]
    );
}

#[test]
fn byte_conversion_uses_text_body_without_debug_boilerplate_when_no_table_is_inferred() {
    let input_path = temp_file_path("minimum_conversion_browser_text", "pdf");
    let output_docx = temp_file_path("minimum_conversion_browser_text_out", "docx");
    fs::write(&input_path, minimal_pdf_with_text("Browser Body Text"))
        .expect("failed to write minimal pdf");
    let pdf_bytes = fs::read(&input_path).expect("minimal pdf must be readable");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("browser-input.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let text_nodes = text_nodes_in_docx(&output_docx);
    assert_eq!(text_nodes, vec!["Browser Body Text".to_string()]);
    assert!(
        !text_nodes
            .iter()
            .any(|text| text.contains("minimum pdf to docx conversion")),
        "browser byte conversion must not emit debug boilerplate"
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn byte_conversion_without_text_or_table_omits_debug_boilerplate() {
    let input_path = temp_file_path("minimum_conversion_browser_empty", "pdf");
    let output_docx = temp_file_path("minimum_conversion_browser_empty_out", "docx");
    fs::write(&input_path, minimal_pdf_without_text()).expect("failed to write minimal pdf");
    let pdf_bytes = fs::read(&input_path).expect("minimal pdf must be readable");

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("browser-input.pdf", &pdf_bytes)
        .expect("byte conversion must succeed");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let text_nodes = text_nodes_in_docx(&output_docx);
    assert_eq!(text_nodes, vec!["(no extractable content)".to_string()]);
    assert!(
        !text_nodes
            .iter()
            .any(|text| text.contains("minimum pdf to docx conversion") || text.contains("source:")),
        "browser byte conversion must not emit debug boilerplate"
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}

#[test]
fn rendered_png_pages_are_embedded_as_docx_images() {
    let output_docx = temp_file_path("rendered_png_pages", "docx");
    let page = RenderedPageImage::new(one_by_one_png_bytes(), 800, 1000);

    let docx_bytes = convert_rendered_png_pages_to_docx_bytes(&[page]).expect("rendered page docx");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert!(doc_xml.contains("<w:drawing>"));
    assert!(doc_xml.contains("rIdImage"));
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="12000" w:h="15000" />"#),
        "DOCX page size should match the rendered PDF page display size"
    );
    assert!(
        doc_xml.contains(
            r#"<w:pgMar w:top="0" w:right="0" w:bottom="0" w:left="0" w:header="0" w:footer="0" w:gutter="0" />"#
        ),
        "rendered page output should use zero margins so the PDF page fills the Word page"
    );
    assert!(
        doc_xml.contains(r#"<wp:extent cx="7620000" cy="9525000" />"#),
        "image extent should use the requested display size"
    );
    assert_eq!(count_media_files_in_docx(&output_docx), 1);
    assert!(read_document_relationships_xml(&output_docx).contains("/image"));

    let _ = fs::remove_file(output_docx);
}

#[test]
fn rendered_png_pages_preserve_page_order_with_page_breaks() {
    let output_docx = temp_file_path("rendered_png_two_pages", "docx");
    let pages = vec![
        RenderedPageImage::new(one_by_one_png_bytes(), 800, 1000),
        RenderedPageImage::new(one_by_one_png_bytes(), 800, 1000),
    ];

    let docx_bytes = convert_rendered_png_pages_to_docx_bytes(&pages).expect("rendered page docx");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert_eq!(doc_xml.matches("<w:drawing>").count(), 2);
    assert_eq!(doc_xml.matches(r#"<w:br w:type="page" />"#).count(), 1);
    assert!(
        count_media_files_in_docx(&output_docx) >= 1,
        "DOCX should contain at least one embedded rendered page image"
    );

    let _ = fs::remove_file(output_docx);
}

#[test]
fn rendered_png_pages_can_preserve_mixed_page_sizes() {
    let output_docx = temp_file_path("rendered_png_mixed_pages", "docx");
    let pages = vec![
        RenderedPageImage::new(one_by_one_png_bytes(), 800, 1000),
        RenderedPageImage::new(one_by_one_png_bytes(), 1000, 800),
    ];

    let docx_bytes =
        convert_rendered_png_pages_to_docx_bytes(&pages).expect("mixed rendered page docx");
    fs::write(&output_docx, docx_bytes).expect("failed to write generated docx");

    let doc_xml = support::docx_assert::read_document_xml(&output_docx);
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="12000" w:h="15000" />"#),
        "first page section should keep portrait size"
    );
    assert!(
        doc_xml.contains(r#"<w:pgSz w:w="15000" w:h="12000" />"#),
        "second page section should keep landscape size"
    );
    assert_eq!(doc_xml.matches("<w:drawing>").count(), 2);

    let _ = fs::remove_file(output_docx);
}

#[test]
fn rendered_png_pages_reject_empty_input() {
    let err = convert_rendered_png_pages_to_docx_bytes(&[]).expect_err("empty pages must fail");
    assert!(
        err.to_string().contains("at least one rendered page"),
        "unexpected error: {err}"
    );
}

#[test]
fn rendered_png_pages_reject_zero_dimensions() {
    let page = RenderedPageImage::new(one_by_one_png_bytes(), 0, 1000);
    let err = convert_rendered_png_pages_to_docx_bytes(&[page]).expect_err("zero width must fail");
    assert!(
        err.to_string().contains("zero dimensions"),
        "unexpected error: {err}"
    );
}

#[test]
fn rendered_png_pages_reject_non_png_bytes() {
    let page = RenderedPageImage::new(b"not a png".to_vec(), 800, 1000);
    let err = convert_rendered_png_pages_to_docx_bytes(&[page]).expect_err("non-png must fail");
    assert!(
        err.to_string().contains("not a PNG"),
        "unexpected error: {err}"
    );
}

fn one_by_one_png_bytes() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xff, 0xff, 0x3f, 0x00, 0x05, 0xfe, 0x02, 0xfe, 0xdc, 0xcc, 0x59, 0xe7, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]
}

fn minimal_pdf_with_image_xobject() -> Vec<u8> {
    b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R >> endobj
4 0 obj << /Length 8 >> stream
/Im1 Do
endstream endobj
5 0 obj << /Type /XObject /Subtype /Image /Width 32 /Height 24 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 0 >> stream
endstream endobj
%%EOF
"
    .to_vec()
}

fn minimal_pdf_with_flate_rgb_image_xobject() -> Vec<u8> {
    let image_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    minimal_pdf_with_flate_image_xobject(&image_stream, 2, 1, "/DeviceRGB", "")
}

fn minimal_pdf_with_many_flate_image_xobjects(count: usize) -> Vec<u8> {
    let image_stream = zlib_compress(&[255, 0, 0, 0, 0, 255]);
    let mut resources = String::new();
    let mut content = String::new();
    for index in 0..count {
        let object_number = 5 + index;
        resources.push_str(&format!("/Im{index} {object_number} 0 R "));
        let x = 10 + (index % 7) * 20;
        let y = 10 + (index / 7) * 20;
        content.push_str(&format!("q 10 0 0 10 {x} {y} cm /Im{index} Do Q\n"));
    }
    let content_stream = zlib_compress(content.as_bytes());
    let mut pdf = format!(
        "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << {resources} >> >> /Contents 4 0 R >> endobj\n4 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
        content_stream.len()
    )
    .into_bytes();
    pdf.extend_from_slice(&content_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    for index in 0..count {
        let object_number = 5 + index;
        pdf.extend_from_slice(
            format!(
                "{object_number} 0 obj << /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >> stream\n",
                image_stream.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&image_stream);
        pdf.extend_from_slice(b"\nendstream endobj\n");
    }
    pdf.extend_from_slice(b"%%EOF\n");
    pdf
}

fn minimal_pdf_with_flate_image_xobject(
    image_stream: &[u8],
    width: u32,
    height: u32,
    color_space: &str,
    extra_dict: &str,
) -> Vec<u8> {
    minimal_pdf_with_flate_image_xobject_and_content(
        image_stream,
        width,
        height,
        color_space,
        extra_dict,
        b"/Im1 Do",
    )
}

fn minimal_pdf_with_flate_image_xobject_and_content(
    image_stream: &[u8],
    width: u32,
    height: u32,
    color_space: &str,
    extra_dict: &str,
    content: &[u8],
) -> Vec<u8> {
    let content_stream = zlib_compress(content);
    let mut pdf = b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R >> endobj
"
    .to_vec();
    pdf.extend_from_slice(
        format!(
            "4 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
            content_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&content_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    pdf.extend_from_slice(
        format!(
            "5 0 obj << /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace {color_space} /BitsPerComponent 8 /Filter /FlateDecode {extra_dict} /Length {} >> stream\n",
            image_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&image_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    if color_space.contains("6 0 R") {
        pdf.extend_from_slice(b"6 0 obj <ff00000000ff> endobj\n");
    }
    pdf.extend_from_slice(b"%%EOF\n");
    pdf
}

fn minimal_pdf_with_flate_smask_image_xobject(
    rgb_stream: &[u8],
    mask_stream: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
    minimal_pdf_with_flate_smask_image_xobject_with_mask_size(
        rgb_stream,
        mask_stream,
        width,
        height,
        width,
        height,
    )
}

fn minimal_pdf_with_flate_smask_image_xobject_with_mask_size(
    rgb_stream: &[u8],
    mask_stream: &[u8],
    width: u32,
    height: u32,
    mask_width: u32,
    mask_height: u32,
) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R >> endobj
4 0 obj << /Length 8 >> stream
/Im1 Do
endstream endobj
"
    .to_vec();
    pdf.extend_from_slice(
        format!(
            "5 0 obj << /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask 6 0 R /Length {} >> stream\n",
            rgb_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(rgb_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    pdf.extend_from_slice(
        format!(
            "6 0 obj << /Type /XObject /Subtype /Image /Width {mask_width} /Height {mask_height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >> stream\n",
            mask_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(mask_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n%%EOF\n");
    pdf
}

fn minimal_pdf_with_flate_smask_image_xobject_with_mask_colorspace_ref(
    rgb_stream: &[u8],
    mask_stream: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R >> endobj
4 0 obj << /Length 8 >> stream
/Im1 Do
endstream endobj
"
    .to_vec();
    pdf.extend_from_slice(
        format!(
            "5 0 obj << /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask 6 0 R /Length {} >> stream\n",
            rgb_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(rgb_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    pdf.extend_from_slice(
        format!(
            "6 0 obj << /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace 7 0 R /BitsPerComponent 8 /Filter /FlateDecode /Length {} >> stream\n",
            mask_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(mask_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n7 0 obj /DeviceGray endobj\n%%EOF\n");
    pdf
}

fn minimal_pdf_with_raw_image_xobject(
    image_stream: &[u8],
    width: u32,
    height: u32,
    color_space: &str,
    filter: &str,
) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R >> endobj
4 0 obj << /Length 8 >> stream
/Im1 Do
endstream endobj
"
    .to_vec();
    pdf.extend_from_slice(
        format!(
            "5 0 obj << /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace {color_space} /BitsPerComponent 1 /Filter {filter} /Length {} >> stream\n",
            image_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(image_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n%%EOF\n");
    pdf
}

fn minimal_pdf_with_unplaced_image_xobject() -> Vec<u8> {
    let unused_image_stream = zlib_compress(&[255, 0, 0]);
    let used_image_stream = zlib_compress(&[0, 0, 255]);
    let mut pdf = b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R /Im2 6 0 R >> >> /Contents 4 0 R >> endobj
4 0 obj << /Length 8 >> stream
/Im2 Do
endstream endobj
"
    .to_vec();
    pdf.extend_from_slice(
        format!(
            "5 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >> stream\n",
            unused_image_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&unused_image_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    pdf.extend_from_slice(
        format!(
            "6 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >> stream\n",
            used_image_stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&used_image_stream);
    pdf.extend_from_slice(b"\nendstream endobj\n%%EOF\n");
    pdf
}

fn ccitt_group4_stream_from_rows(rows: &[&[bool]]) -> Vec<u8> {
    let width = rows.first().expect("at least one row").len() as u16;
    let writer = VecWriter::new();
    let mut encoder = Encoder::new(writer);
    for row in rows {
        let pels = row
            .iter()
            .map(|black| if *black { Color::Black } else { Color::White });
        encoder
            .encode_line(pels, width)
            .expect("CCITT line encode must succeed");
    }
    let mut writer = encoder.finish().expect("CCITT finish must succeed");
    writer.pad();
    writer.finish()
}

fn ccitt_group3_1d_stream_from_rows(rows: &[&[bool]]) -> Vec<u8> {
    let mut writer = VecWriter::new();
    writer.write(EOL).expect("initial EOL must write");
    for row in rows {
        write_ccitt_group3_row(&mut writer, row);
        writer.write(EOL).expect("line EOL must write");
    }
    for _ in 0..5 {
        writer.write(EOL).expect("RTC EOL must write");
    }
    writer.pad();
    writer.finish()
}

fn ccitt_group3_1d_byte_aligned_stream_from_rows(rows: &[&[bool]]) -> Vec<u8> {
    let mut writer = VecWriter::new();
    writer.pad();
    writer.write(EOL).expect("initial EOL must write");
    for row in rows {
        write_ccitt_group3_row(&mut writer, row);
        writer.pad();
        writer.write(EOL).expect("line EOL must write");
    }
    for _ in 0..5 {
        writer.pad();
        writer.write(EOL).expect("RTC EOL must write");
    }
    writer.pad();
    writer.finish()
}

fn write_ccitt_group3_row(writer: &mut VecWriter, row: &[bool]) {
    let mut color = false;
    let mut run = 0u16;
    for black in row {
        if *black == color {
            run += 1;
        } else {
            write_ccitt_group3_run(writer, color, run);
            color = *black;
            run = 1;
        }
    }
    write_ccitt_group3_run(writer, color, run);
}

fn write_ccitt_group3_run(writer: &mut VecWriter, black_run: bool, mut run: u16) {
    while run >= 2560 {
        let bits = if black_run {
            black::encode(2560)
        } else {
            white::encode(2560)
        }
        .expect("CCITT makeup code must exist");
        writer.write(bits).expect("CCITT makeup must write");
        run -= 2560;
    }
    if run >= 64 {
        let makeup = run & !63;
        let bits = if black_run {
            black::encode(makeup)
        } else {
            white::encode(makeup)
        }
        .expect("CCITT makeup code must exist");
        writer.write(bits).expect("CCITT makeup must write");
        run -= makeup;
    }
    let bits: Bits = if black_run {
        black::encode(run)
    } else {
        white::encode(run)
    }
    .expect("CCITT terminal code must exist");
    writer.write(bits).expect("CCITT terminal must write");
}

fn zlib_compress(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("zlib write must succeed");
    encoder.finish().expect("zlib finish must succeed")
}

fn flate_pdf_with_page_contents(contents: &[&[u8]]) -> Vec<u8> {
    flate_pdf_with_page_contents_and_base_font(contents, "Helvetica")
}

fn flate_pdf_with_page_contents_and_base_font(contents: &[&[u8]], base_font: &str) -> Vec<u8> {
    flate_pdf_with_page_contents_and_base_fonts(contents, &[("F1", base_font)])
}

fn flate_pdf_with_page_contents_and_base_fonts(
    contents: &[&[u8]],
    fonts: &[(&str, &str)],
) -> Vec<u8> {
    let page_sizes = vec![(300, 200); contents.len()];
    flate_pdf_with_page_contents_base_fonts_and_page_sizes(contents, fonts, &page_sizes)
}

/// ページごとの MediaBox 寸法を指定した Flate content stream PDF を作る。
fn flate_pdf_with_page_contents_base_fonts_and_page_sizes(
    contents: &[&[u8]],
    fonts: &[(&str, &str)],
    page_sizes: &[(u32, u32)],
) -> Vec<u8> {
    let rotations = vec![0; contents.len()];
    flate_pdf_with_page_contents_base_fonts_page_sizes_and_rotations(
        contents, fonts, page_sizes, &rotations,
    )
}

/// ページごとの MediaBox 寸法と回転角度を指定した Flate content stream PDF を作る。
fn flate_pdf_with_page_contents_base_fonts_page_sizes_and_rotations(
    contents: &[&[u8]],
    fonts: &[(&str, &str)],
    page_sizes: &[(u32, u32)],
    rotations: &[i32],
) -> Vec<u8> {
    let crop_boxes = vec![None; contents.len()];
    flate_pdf_with_page_contents_base_fonts_page_sizes_rotations_and_crop_boxes(
        contents,
        fonts,
        page_sizes,
        rotations,
        &crop_boxes,
    )
}

/// MediaBox と CropBox を指定した単一ページ Flate content stream PDF を作る。
fn flate_pdf_with_page_content_and_crop_box(
    content: &[u8],
    page_size: (u32, u32),
    crop_box: (i32, i32, i32, i32),
) -> Vec<u8> {
    flate_pdf_with_page_contents_base_fonts_page_sizes_rotations_and_crop_boxes(
        &[content],
        &[("F1", "Helvetica")],
        &[page_size],
        &[0],
        &[Some(crop_box)],
    )
}

/// Pdfiumでも開けるxref付き単一ページCropBox PDFを作る。
fn valid_pdf_with_page_content_and_crop_box(
    content: &[u8],
    page_size: (u32, u32),
    crop_box: (i32, i32, i32, i32),
) -> Vec<u8> {
    valid_pdf_with_page_content_and_box_placement(content, page_size, crop_box, false, None)
}

/// 親Pages nodeにMediaBoxとCropBoxを持つxref付きPDFを作る。
fn valid_pdf_with_inherited_page_boxes(
    content: &[u8],
    page_size: (u32, u32),
    crop_box: (i32, i32, i32, i32),
) -> Vec<u8> {
    valid_pdf_with_page_content_and_box_placement(content, page_size, crop_box, true, None)
}

/// 親Pages nodeにMediaBox、CropBox、Rotateを持つxref付きPDFを作る。
fn valid_pdf_with_inherited_page_boxes_and_rotation(
    content: &[u8],
    page_size: (u32, u32),
    crop_box: (i32, i32, i32, i32),
    rotation: i32,
) -> Vec<u8> {
    valid_pdf_with_page_content_and_box_placement(
        content,
        page_size,
        crop_box,
        true,
        Some(rotation),
    )
}

/// page boxの配置先を選べるxref付き単一ページPDFを作る。
fn valid_pdf_with_page_content_and_box_placement(
    content: &[u8],
    page_size: (u32, u32),
    crop_box: (i32, i32, i32, i32),
    boxes_on_parent: bool,
    parent_rotation: Option<i32>,
) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize];
    let boxes = format!(
        "/MediaBox [0 0 {} {}] /CropBox [{} {} {} {}]",
        page_size.0, page_size.1, crop_box.0, crop_box.1, crop_box.2, crop_box.3,
    );
    let pages_boxes = boxes_on_parent.then_some(boxes.as_str()).unwrap_or("");
    let page_boxes = (!boxes_on_parent).then_some(boxes.as_str()).unwrap_or("");
    let pages_rotation = parent_rotation
        .map(|rotation| format!("/Rotate {rotation}"))
        .unwrap_or_default();
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [3 0 R] /Count 1 {pages_boxes} {pages_rotation} >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /Page /Parent 2 0 R {page_boxes} /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        )
        .into_bytes(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).into_bytes(),
            content.to_vec(),
            b"\nendstream".to_vec(),
        ]
        .concat(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    for (index, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", offsets.len()).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    pdf
}

/// ページごとのMediaBox、回転角度、任意のCropBoxを指定したPDFを作る。
fn flate_pdf_with_page_contents_base_fonts_page_sizes_rotations_and_crop_boxes(
    contents: &[&[u8]],
    fonts: &[(&str, &str)],
    page_sizes: &[(u32, u32)],
    rotations: &[i32],
    crop_boxes: &[Option<(i32, i32, i32, i32)>],
) -> Vec<u8> {
    assert_eq!(contents.len(), page_sizes.len());
    assert_eq!(contents.len(), rotations.len());
    assert_eq!(contents.len(), crop_boxes.len());
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
    let kids = (0..contents.len())
        .map(|index| format!("{} 0 R", 3 + index))
        .collect::<Vec<_>>()
        .join(" ");
    pdf.extend_from_slice(
        format!(
            "2 0 obj << /Type /Pages /Kids [{kids}] /Count {} >> endobj\n",
            contents.len()
        )
        .as_bytes(),
    );
    for (index, (page_width, page_height)) in page_sizes.iter().copied().enumerate() {
        let page_object = 3 + index;
        let content_object = 3 + contents.len() + index;
        let font_resources = fonts
            .iter()
            .enumerate()
            .map(|(font_index, (resource, _))| format!("/{resource} {} 0 R", 99 + font_index))
            .collect::<Vec<_>>()
            .join(" ");
        let rotation = rotations[index];
        let rotation_entry = (rotation != 0)
            .then(|| format!(" /Rotate {rotation}"))
            .unwrap_or_default();
        let crop_box_entry = crop_boxes[index]
            .map(|(x0, y0, x1, y1)| format!(" /CropBox [{x0} {y0} {x1} {y1}]"))
            .unwrap_or_default();
        pdf.extend_from_slice(
            format!(
                "{page_object} 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_width} {page_height}]{crop_box_entry}{rotation_entry} /Contents {content_object} 0 R /Resources << /Font << {font_resources} >> >> >> endobj\n"
            )
            .as_bytes(),
        );
    }
    for (index, content) in contents.iter().enumerate() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(content).expect("zlib write must succeed");
        let compressed = encoder.finish().expect("zlib finish must succeed");
        let object_number = 3 + contents.len() + index;
        pdf.extend_from_slice(
            format!(
                "{object_number} 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
                compressed.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&compressed);
        pdf.extend_from_slice(b"\nendstream endobj\n");
    }
    for (font_index, (_, base_font)) in fonts.iter().enumerate() {
        pdf.extend_from_slice(
            format!(
                "{} 0 obj << /Type /Font /Subtype /Type1 /BaseFont /{base_font} >> endobj\n",
                99 + font_index
            )
            .as_bytes(),
        );
    }
    pdf.extend_from_slice(b"%%EOF\n");
    pdf
}

fn horizontal(y: f32) -> ParsedGraphicElement {
    ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 0.0,
            y0: y,
            x1: 160.0,
            y1: y,
        },
        stroke_width: 0.5,
    }
}

fn vertical(x: f32) -> ParsedGraphicElement {
    ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: x,
            y0: 0.0,
            x1: x,
            y1: 100.0,
        },
        stroke_width: 0.5,
    }
}

fn text_element(x: f32, y: f32, value: &str) -> ParsedTextElement {
    text_element_on_page(0, x, y, value)
}

fn text_element_on_page(page: usize, x: f32, y: f32, value: &str) -> ParsedTextElement {
    ParsedTextElement {
        page,
        bounds: ParsedBounds {
            x0: x,
            y0: y,
            x1: x + 10.0,
            y1: y + 10.0,
        },
        font_size: 10.0,
        font_name: None,
        fill_color: None,
        text: value.to_string(),
    }
}

fn image_element_on_page(page: usize) -> ParsedImageElement {
    ParsedImageElement {
        page,
        bounds: ParsedBounds {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 100.0,
        },
        png_bytes: b"\x89PNG\r\n\x1a\n".to_vec(),
        width_px: 100,
        height_px: 100,
        floating: false,
        rotation_degrees: 0,
    }
}

fn parsed_document_for_auto_ocr(
    page_count: usize,
    text_elements: Vec<ParsedTextElement>,
    image_elements: Vec<ParsedImageElement>,
    text_preview: String,
) -> ParsedDocument {
    ParsedDocument {
        quality: Default::default(),
        path: "auto-ocr.pdf".into(),
        byte_len: 1,
        header: "%PDF-1.7".to_string(),
        version: PdfVersion { major: 1, minor: 7 },
        page_count,
        pages: (0..page_count)
            .map(|page_number| ParsedPage {
                page_number,
                width: 100.0,
                height: 100.0,
                rotation: 0,
            })
            .collect(),
        graphics: Vec::new(),
        text_elements,
        image_elements,
        embedded_fonts: Vec::new(),
        text_preview,
    }
}

fn png_rgb_has_non_white_pixel(bytes: &[u8]) -> bool {
    if bytes.len() < 33 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return false;
    }
    let mut cursor = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut color_type = 0u8;
    let mut idat = Vec::new();
    while cursor + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        let kind = &bytes[cursor + 4..cursor + 8];
        let data_start = cursor + 8;
        let data_end = data_start.saturating_add(length);
        if data_end + 4 > bytes.len() {
            return false;
        }
        match kind {
            b"IHDR" => {
                if length < 13 {
                    return false;
                }
                width = u32::from_be_bytes(bytes[data_start..data_start + 4].try_into().unwrap());
                height =
                    u32::from_be_bytes(bytes[data_start + 4..data_start + 8].try_into().unwrap());
                color_type = bytes[data_start + 9];
            }
            b"IDAT" => idat.extend_from_slice(&bytes[data_start..data_end]),
            b"IEND" => break,
            _ => {}
        }
        cursor = data_end + 4;
    }
    let channels = match color_type {
        2 => 3usize,
        6 => 4usize,
        _ => return false,
    };
    let row_len = width as usize * channels;
    let mut decoder = ZlibDecoder::new(idat.as_slice());
    let mut scanlines = Vec::new();
    if decoder.read_to_end(&mut scanlines).is_err() {
        return false;
    }
    for row in 0..height as usize {
        let start = row * (row_len + 1);
        if scanlines.get(start) != Some(&0) {
            return false;
        }
        let row_pixels =
            &scanlines[start + 1..start + 1 + row_len.min(scanlines.len() - start - 1)];
        if row_pixels
            .chunks_exact(channels)
            .any(|pixel| pixel[0] != 255 || pixel[1] != 255 || pixel[2] != 255)
        {
            return true;
        }
    }
    false
}

#[test]
fn minimum_conversion_clean_output_emits_only_ocr_paragraphs() {
    let input_path = temp_file_path("minimum_conversion_clean", "pdf");
    let output_docx = temp_file_path("minimum_conversion_clean_out", "docx");
    fs::write(&input_path, minimal_pdf_without_text()).expect("failed to write minimal pdf");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_path.clone(),
        output_docx_path: output_docx.clone(),
        ocr_text_lines: Some(vec!["OnlyOne".to_string(), "OnlyTwo".to_string()]),
        ocr_table: None,
        clean_output: true,
    };
    convert_minimum_pdf_to_docx(&request).expect("minimum conversion must succeed");

    let file = std::fs::File::open(&output_docx).expect("failed to open docx");
    let mut zip = zip::ZipArchive::new(file).expect("failed to open docx zip");
    let mut doc_xml = String::new();
    std::io::Read::read_to_string(
        &mut zip
            .by_name("word/document.xml")
            .expect("missing word/document.xml"),
        &mut doc_xml,
    )
    .expect("failed to read document.xml");
    assert!(doc_xml.contains("OnlyOne"));
    assert!(doc_xml.contains("OnlyTwo"));
    assert!(
        !doc_xml.contains("minimum pdf to docx conversion"),
        "clean output must omit debug boilerplate paragraphs"
    );

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_docx);
}
