//! 原本の罫線小数座標からWord表寸法を求め、整数ptの推定キーと分離する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 99.5ptと101ptの列、29.5ptと30.5ptの行を持つ正式な二行二列表を生成する。
fn source() -> Vec<u8> {
    let content = "1 w 72.25 590.25 m 272.75 590.25 l S 72.25 620.75 m 272.75 620.75 l S 72.25 650.25 m 272.75 650.25 l S 72.25 590.25 m 72.25 650.25 l S 171.75 590.25 m 171.75 650.25 l S 272.75 590.25 m 272.75 650.25 l S BT /F 10 Tf 82 633 Td (Alpha) Tj ET BT /F 10 Tf 82 603 Td (Beta) Tj ET BT /F 10 Tf 182 603 Td (Control) Tj ET";
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 列幅は推定キーの100/101ptでなく、原本の99.5/101ptからtwipへ一度だけ丸める。
#[test]
fn editable_table_keeps_fractional_source_column_widths() {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-fractional-table.pdf", &source()).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    assert!(xml.contains("Alpha</w:t>") && xml.contains("Beta</w:t>") && xml.contains("Control</w:t>"));
    assert!(xml.contains("<w:gridCol w:w=\"1990\""), "the first column must use its 99.5pt source interval: {xml}");
    assert!(xml.contains("<w:gridCol w:w=\"2020\""));
}
