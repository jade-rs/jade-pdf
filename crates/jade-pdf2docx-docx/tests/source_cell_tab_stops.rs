//! 同じセルの離れた文字群を、空白数の推測でなく原本位置へ保つ。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 二行二列表の一セルに、60pt離れた同じbaselineの文字群を描く。
fn source() -> Vec<u8> {
    let content = "1 w 72 590 m 372 590 l S 72 620 m 372 620 l S 72 650 m 372 650 l S 72 590 m 72 650 l S 272 590 m 272 650 l S 372 590 m 372 650 l S BT /F 10 Tf 82 633 Td (New) Tj ET BT /F 10 Tf 142 633 Td (Old) Tj ET BT /F 10 Tf 82 603 Td (Control) Tj ET BT /F 10 Tf 282 603 Td (End) Tj ET";
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 製品APIから得たWord XMLを、字形・空白・位置の書換えなしに読む。
fn xml() -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-tabs.pdf", &source()).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    out
}

/// セル左端から70ptの後続文字原点を明示し、二つの数値群を詰めない。
#[test]
fn separated_cell_groups_keep_their_source_tab_position() {
    let s = xml();
    assert_eq!(s.matches("<w:tbl>").count(), 1);
    assert!(s.contains("w:val=\"left\" w:pos=\"1400\""), "{s}");
    assert!(s.contains("<w:tab/>"), "{s}");
    for text in ["New", "Old", "Control", "End"] {
        assert_eq!(s.matches(&format!(">{text}</w:t>")).count(), 1);
    }
}
