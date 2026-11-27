//! 横罫線だけの原本を、列幅と縦罫線の欠落を保つ編集可能なWord表へ変える。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 横罫線4本と、罫線内の明示的な本文配置を持つ所有PDFを作る。
fn source(body: &str) -> Vec<u8> {
    let content = format!(
        "0.5 w 20 160 m 220 160 l S 20 140 m 220 140 l S \
         20 120 m 220 120 l S 20 100 m 220 100 l S {body}"
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ])
}

/// 公開変換APIが生成した本文XMLを読み取る。
fn document_xml(body: &str) -> String {
    let docx = convert_minimum_pdf_bytes_to_docx_bytes("owned-horizontal-rules.pdf", &source(body))
        .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(docx)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 欄外の見出しと、左寄せ・右寄せが混在する三列の内容。
const TABLE_CONTENT: &str = "BT /F1 10 Tf 1 0 0 1 20 175 Tm (Outside caption) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 146 Tm (Year) Tj ET \
         BT /F1 10 Tf 1 0 0 1 95 146 Tm (Sales) Tj ET \
         BT /F1 10 Tf 1 0 0 1 165 146 Tm (Profit) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 126 Tm (2021) Tj ET \
         BT /F1 10 Tf 1 0 0 1 107 126 Tm (100) Tj ET \
         BT /F1 10 Tf 1 0 0 1 189 126 Tm (7) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 106 Tm (2022) Tj ET \
         BT /F1 10 Tf 1 0 0 1 107 106 Tm (200) Tj ET \
         BT /F1 10 Tf 1 0 0 1 183 106 Tm (14) Tj ET";

/// 表の見出し・位置の異なる数値・欄外本文を残し、原本の横罫線だけを出力する。
#[test]
fn horizontal_rules_keep_the_editor_grid_and_never_add_vertical_borders() {
    let xml = document_xml(TABLE_CONTENT);
    assert_eq!(xml.matches("<w:tbl>").count(), 1, "{xml}");
    let (before_table, table) = xml.split_once("<w:tbl>").unwrap();
    let table = table.split_once("</w:tbl>").unwrap().0;
    assert!(before_table.contains(">Outside caption<"), "{before_table}");
    assert_eq!(table.matches("<w:tr>").count(), 3, "{table}");
    assert_eq!(table.matches("<w:tc>").count(), 9, "{table}");
    assert_eq!(
        table.matches(r#"<w:gridCol w:w="1400""#).count(),
        2,
        "{table}"
    );
    assert_eq!(
        table.matches(r#"<w:gridCol w:w="1200""#).count(),
        1,
        "{table}"
    );
    for (edge, value) in [
        ("top", "single"),
        ("bottom", "single"),
        ("left", "nil"),
        ("right", "nil"),
    ] {
        let border = format!(r#"<w:{edge} w:val="{value}""#);
        assert_eq!(table.matches(&border).count(), 9, "{edge}: {table}");
    }
    for text in [
        "Year", "Sales", "Profit", "2021", "100", "7", "2022", "200", "14",
    ] {
        assert_eq!(
            xml.matches(&format!(">{text}<")).count(),
            1,
            "{text}: {xml}"
        );
    }
}

/// 空の金額欄も、前後の行から確認できた独立列として残す。
#[test]
fn horizontal_rules_keep_an_empty_cell_in_its_original_column() {
    let content = TABLE_CONTENT.replace("BT /F1 10 Tf 1 0 0 1 107 126 Tm (100) Tj ET", "");
    let xml = document_xml(&content);
    let table = xml
        .split_once("<w:tbl>")
        .unwrap()
        .1
        .split_once("</w:tbl>")
        .unwrap()
        .0;
    assert_eq!(table.matches("<w:gridCol ").count(), 3, "{table}");
    let middle_row = table.split("<w:tr>").nth(2).unwrap();
    let cells = middle_row.split("<w:tc>").skip(1).collect::<Vec<_>>();
    assert_eq!(cells.len(), 3);
    assert!(cells[0].contains(">2021<"), "{}", cells[0]);
    let text = cells[1]
        .split("</w:t>")
        .filter_map(|part| {
            let tag = part.rsplit_once("<w:t")?.1;
            matches!(tag.as_bytes().first(), Some(b' ' | b'>')).then_some(tag.split_once('>')?.1)
        })
        .collect::<String>();
    assert!(text.is_empty(), "{}", cells[1]);
    assert!(cells[2].contains(">7<"), "{}", cells[2]);
    assert!(!middle_row.contains("<w:gridSpan "), "{middle_row}");
}

/// 罫線の間に一列しかない散文を、列の繰り返しがある表として扱わない。
#[test]
fn aligned_rules_without_repeated_columns_keep_prose() {
    let xml = document_xml(
        "BT /F1 10 Tf 1 0 0 1 25 146 Tm (First line of ordinary prose.) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 126 Tm (Second line of ordinary prose.) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 106 Tm (Third line of ordinary prose.) Tj ET",
    );
    assert!(!xml.contains("<w:tbl>"), "{xml}");
    assert!(xml.contains(">Third line of ordinary prose.<"), "{xml}");
}
