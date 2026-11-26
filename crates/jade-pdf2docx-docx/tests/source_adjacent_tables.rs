//! 横並びの独立した原本表を、編集可能な二つのWord表のまま配置する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 二行二列の表二つと、表後の本文を持つ所有原本。
fn source() -> Vec<u8> {
    let mut content = String::from("BT /F1 8 Tf 30 380 Td (Heading) Tj ET ");
    for (x, prefix) in [(30, "A"), (160, "B")] {
        for y in [300, 330, 360] { content.push_str(&format!("{x} {y} m {} {y} l S ", x + 100)); }
        for left in [x, x + 50, x + 100] { content.push_str(&format!("{left} 300 m {left} 360 l S ")); }
        for (index, (dx, y)) in [(5, 345), (55, 345), (5, 315), (55, 315)].into_iter().enumerate() {
            content.push_str(&format!("BT /F1 8 Tf {} {y} Td ({prefix}{index}) Tj ET ", x + dx));
        }
    }
    content.push_str("1 g 130 329.5 30 1 re f 0 g BT /F1 8 Tf 30 280 Td (After) Tj ET");
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 300 420] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        page_attribute_pdf::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ])
}

/// 結合・行列・本文を持つ二つの表を、同じ原本の上端へ配置する。
#[test]
fn distinct_source_tables_keep_shared_top_and_editable_cells() {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-adjacent.pdf", &source()).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    assert_eq!(xml.matches("<w:tbl>").count(), 2, "{xml}");
    assert_eq!(xml.matches("<w:tr>").count(), 4);
    for text in ["A0", "A1", "A2", "A3", "B0", "B1", "B2", "B3", "Heading", "After"] {
        assert_eq!(xml.matches(&format!(">{text}</w:t>")).count(), 1, "{text}");
    }
    assert_eq!(xml.matches("<w:tblpPr").count(), 2, "independent tables must share their source top: {xml}");
    assert_eq!(xml.matches("w:tblpY=\"1200\"").count(), 2);
    for x in [600, 3200] { assert!(xml.contains(&format!("w:tblpX=\"{x}\""))); }
}
