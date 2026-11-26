//! 元の空セルの対角線を、向きと線幅を保ったWord罫線へ渡す。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 正式な二行二列の表の右上セルだけへ黒い対角線を引く。
fn source() -> Vec<u8> {
    let content = "0 G .5 w 72 590 m 272 590 l S 72 620 m 272 620 l S 72 650 m 272 650 l S 72 590 m 72 650 l S 172 590 m 172 650 l S 272 590 m 272 650 l S 172 650 m 272 620 l S BT /F 10 Tf 82 633 Td (Alpha) Tj ET BT /F 10 Tf 82 603 Td (Beta) Tj ET BT /F 10 Tf 182 603 Td (Control) Tj ET";
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 黒い斜線を、同じ空セルの左上から右下へ引くWord罫線にする。
#[test]
fn a_source_diagonal_is_preserved_in_its_empty_table_cell() {
    let output=convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-diagonal.pdf",&source()).unwrap();
    let mut zip=zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut xml=String::new();zip.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    assert_eq!(xml.matches("<w:tbl>").count(),1);
    assert!(xml.contains("Alpha</w:t>") && xml.contains("Beta</w:t>") && xml.contains("Control</w:t>"));
    assert_eq!(xml.matches("<w:tl2br ").count(),1,"the actual top-left-to-bottom-right source line must remain in its cell");
}
