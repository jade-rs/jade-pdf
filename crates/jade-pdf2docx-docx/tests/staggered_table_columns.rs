//! 段境界のある横並び表を、Wordでも段順かつ原本座標で保持する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 上端の異なる罫線表2つと、両方を覆う段境界を持つ正式PDFを生成する。
fn source(separator: &str) -> Vec<u8> {
    let mut content = String::from("0 g 0 G .5 w ");
    for (x, y, label) in [(320, 460, "RIGHT"), (40, 400, "LEFT")] {
        for offset in [0, 60, 120] {
            content.push_str(&format!(
                "{x} {} m {} {} l S ",
                y + offset,
                x + 200,
                y + offset
            ));
        }
        for offset in [0, 100, 200] {
            content.push_str(&format!(
                "{} {y} m {} {} l S ",
                x + offset,
                x + offset,
                y + 120
            ));
        }
        for (index, (dx, dy)) in [(10, 90), (110, 90), (10, 30), (110, 30)]
            .into_iter()
            .enumerate()
        {
            content.push_str(&format!(
                "BT /F1 10 Tf {} {} Td ({label} {index}) Tj ET ",
                x + dx,
                y + dy
            ));
        }
    }
    content.push_str(separator);
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 612 792] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 変換したDOCXの本文XMLを取得する。
fn document_xml(input: &[u8]) -> String {
    let output =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-staggered-tables.pdf", input).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 原本の段順と、異なる上下位置の両方を編集可能なWord表として保つ。
#[test]
fn verified_gutter_keeps_table_order_and_page_positions() {
    let xml = document_xml(&source("280 380 m 280 600 l S"));
    assert_eq!(xml.matches("<w:tbl>").count(), 2);
    assert!(xml.find("LEFT 0").unwrap() < xml.find("RIGHT 0").unwrap());
    for anchor in [
        r#"w:tblpX="800""#,
        r#"w:tblpY="5440""#,
        r#"w:tblpX="6400""#,
        r#"w:tblpY="4240""#,
    ] {
        assert!(xml.contains(anchor), "{anchor}: {xml}");
    }
}

/// 表全高の根拠が不足する短い線では、上から下への本文順を保つ。
#[test]
fn partial_rule_keeps_the_existing_flow_order() {
    let xml = document_xml(&source("280 500 m 280 550 l S"));
    assert_eq!(xml.matches("<w:tbl>").count(), 2);
    assert!(xml.find("RIGHT 0").unwrap() < xml.find("LEFT 0").unwrap());
    assert!(!xml.contains("<w:tblpPr"));
}
