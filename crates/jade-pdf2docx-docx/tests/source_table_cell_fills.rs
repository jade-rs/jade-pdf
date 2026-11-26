//! 原本の矩形塗りを、罫線や文字色と区別して編集可能な表セルへ対応させる。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 青い見出し二セルと黄色の数値セルを持つ、所有する正式PDFを構成する。
fn source() -> Vec<u8> {
    let mut content =
        String::from(".2 .4 .6 rg 30 330 100 30 re f 1 1 0 rg 80 300 50 30 re f 0 g 0 G 1 w ");
    for y in [300, 330, 360] {
        content.push_str(&format!("30 {y} m 130 {y} l S "));
    }
    for x in [30, 80, 130] {
        content.push_str(&format!("{x} 300 m {x} 360 l S "));
    }
    for (word, x, y) in [
        ("A0", 35, 345),
        ("A1", 85, 345),
        ("A2", 35, 315),
        ("A3", 85, 315),
    ] {
        content.push_str(&format!("BT /F1 8 Tf {x} {y} Td ({word}) Tj ET "));
    }
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 300 420] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        page_attribute_pdf::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ])
}

/// 公開変換が見出し行の塗りと一セルの強調を別々のセル背景として保持する。
#[test]
fn source_header_and_highlight_fills_belong_to_editable_cells() {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-fills.pdf", &source()).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    assert_eq!(xml.matches("<w:tr>").count(), 2);
    assert_eq!(xml.matches("<w:tc>").count(), 4);
    for (word, color) in [
        ("A0", Some("336699")),
        ("A1", Some("336699")),
        ("A2", None),
        ("A3", Some("FFFF00")),
    ] {
        let cell = xml
            .split("<w:tc>")
            .find(|cell| cell.contains(&format!(">{word}</w:t>")))
            .unwrap()
            .split("</w:tc>")
            .next()
            .unwrap();
        if let Some(color) = color {
            assert!(
                cell.contains(&format!("w:fill=\"{color}\"")),
                "{word} must retain its source background: {cell}"
            );
        } else {
            assert!(
                !cell.contains("<w:shd"),
                "{word} must not inherit neighboring fill"
            );
        }
        assert_eq!(xml.matches(&format!(">{word}</w:t>")).count(), 1);
    }
}
