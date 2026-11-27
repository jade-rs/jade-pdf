//! 不透明なRGBページGroupの表背景を、編集可能なセルの塗りとして残す。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 青い太字見出しと通常の本文を持つ2列2行の表を、外部PDFなしで構築する。
fn source(partial: bool) -> Vec<u8> {
    let fill = if partial {
        "q 10 120 80 20 re W n .553 .702 .886 rg 10 120 180 20 re f* Q 0 g"
    } else {
        ".553 .702 .886 rg 10 120 180 20 re f* 0 g"
    };
    let content = format!(
        "{fill} 1 w 10 100 m 190 100 l S 10 120 m 190 120 l S 10 140 m 190 140 l S 10 100 m 10 140 l S 100 100 m 100 140 l S 190 100 m 190 140 l S q 10 120 90 20 re W* n BT /B 10 Tf 20 125 Td (H1) Tj ET Q q 100 120 90 20 re W* n BT /B 10 Tf 110 125 Td (H2) Tj ET Q BT /F 10 Tf 20 105 Td (A) Tj ET BT /F 10 Tf 110 105 Td (B) Tj ET"
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 200 300] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Group << /Type /Group /S /Transparency /CS /DeviceRGB >> /Resources << /Font << /F 5 0 R /B 6 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".into(),
    ])
}

/// 本文XMLを展開し、画像への置換ではなくセル構造と塗りを検証する。
fn xml(bytes: Vec<u8>) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// ページGroupがあっても青い見出し2セル・太字・通常本文と表の形を保持する。
#[test]
fn opaque_rgb_group_keeps_editable_blue_bold_header_cells() {
    let text = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-page-group-table.pdf",
        &source(false),
    )
    .unwrap());
    assert_eq!(text.matches("<w:tbl>").count(), 1);
    assert_eq!(text.matches("<w:tr>").count(), 2);
    assert_eq!(text.matches("<w:tc>").count(), 4);
    assert_eq!(text.matches("w:fill=\"8DB3E2\"").count(), 2);
    for value in ["H1", "H2", "A", "B"] {
        assert!(text.contains(&format!(">{value}</w:t>")));
    }
    for cell in text.split("<w:tc>").skip(1) {
        let cell = cell.split("</w:tc>").next().unwrap();
        let header = cell.contains(">H1</w:t>") || cell.contains(">H2</w:t>");
        assert_eq!(cell.contains("w:fill=\"8DB3E2\""), header);
        assert_eq!(cell.contains("<w:b/>") || cell.contains("<w:b />"), header);
    }
}

/// 確認できたページGroupでも、部分的な原本塗りをセル全面へ広げない。
#[test]
fn page_group_does_not_expand_a_partially_clipped_fill() {
    let text = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-partial-group-table.pdf",
        &source(true),
    )
    .unwrap());
    assert_eq!(text.matches("w:fill=\"8DB3E2\"").count(), 0);
}

/// nativeファイル変換でも同じセル色を残し、入力原本を変更しない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_path_keeps_the_same_blue_header_cells() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    let dir = tempfile::tempdir().unwrap();
    let bytes = source(false);
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("converted.docx");
    std::fs::write(&input, &bytes).unwrap();
    convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
        input_pdf_path: input.clone(),
        output_docx_path: output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    })
    .unwrap();
    assert_eq!(std::fs::read(input).unwrap(), bytes);
    assert_eq!(
        xml(std::fs::read(output).unwrap())
            .matches("w:fill=\"8DB3E2\"")
            .count(),
        2
    );
}
