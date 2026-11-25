//! 原本の表外本文を、推測したY反転の表所有へ吸い込まない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use std::io::{Cursor, Read};

/// 上半分の自作罫線表と、Yを反転した時だけ表へ入る下半分の本文。
fn source(caption_y: u16) -> Vec<u8> {
    let mut content = String::from("1 w ");
    for y in [550, 600, 650, 700] {
        content.push_str(&format!("72 {y} m 272 {y} l S "));
    }
    for x in [72, 172, 272] {
        content.push_str(&format!("{x} 550 m {x} 700 l S "));
    }
    for (row, y) in [675, 625, 575].into_iter().enumerate() {
        for (col, x) in [82, 182].into_iter().enumerate() {
            content.push_str(&format!(
                "BT /F1 10 Tf {x} {y} Td (Cell-{row}-{col}) Tj ET "
            ));
        }
    }
    content.push_str(&format!(
        "BT /F1 10 Tf 82 {caption_y} Td (Outside source caption) Tj ET"
    ));
    owned_pdf_content(&content)
}

/// 解析済みの原本座標で表へ所属していない本文を、DOCXにも一度だけ保持する。
#[test]
fn source_table_does_not_swallow_reflected_outside_caption() {
    let xml = document_xml(&source(150));
    assert!(
        xml.contains("<w:tbl>"),
        "the genuine source table must remain editable"
    );
    assert_eq!(
        xml.matches("Outside source caption").count(),
        1,
        "outside text is not owned by the reflected table"
    );
    for row in 0..3 {
        for col in 0..2 {
            assert_eq!(xml.matches(&format!("Cell-{row}-{col}")).count(), 1);
        }
    }
}

/// 内容streamから既存と同じobject順・xref形式を生成し、所有対照のbyteを保つ。
fn owned_pdf_content(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 公開byte APIの出力本文XMLを、閉じたDOCX archiveから取り出す。
fn document_xml(source: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-logical-rows.pdf", source).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/source_table_text_ownership.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 表の鏡像領域に入る全ての下半分の本文baselineでも、原文を一度だけ保つ。
    #[test]
    fn reflected_caption_positions_preserve_text(caption_y in 100u16..230) {
        let xml = document_xml(&source(caption_y));
        prop_assert!(xml.contains("<w:tbl>"));
        prop_assert_eq!(xml.matches("Outside source caption").count(), 1);
        for row in 0..3 {
            for col in 0..2 {
                prop_assert_eq!(xml.matches(&format!("Cell-{row}-{col}")).count(), 1);
            }
        }
    }
}

/// 表と鏡像が交わらない上側の本文も、同じ原本所有判定で保持する。
#[test]
fn above_table_caption_is_preserved() {
    let xml = document_xml(&source(740));
    assert_eq!(xml.matches("Outside source caption").count(), 1);
    assert!(xml.contains("<w:tbl>"));
}
