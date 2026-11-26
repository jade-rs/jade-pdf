//! TJの明示間隔で離れた原本数値を、Wordの別セルへ割り当てる。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 実在する罫線gridの列原点に、単一TJ命令から別々の数値を描く所有原本。
fn source() -> Vec<u8> {
    owned_pdf_content(
        "1 w 72 600 m 272 600 l S 72 650 m 272 650 l S 72 700 m 272 700 l S 72 600 m 72 700 l S 172 600 m 172 700 l S 272 600 m 272 700 l S BT /F1 10 Tf 82 675 Td [(376) -8200 (-2.4)] TJ ET BT /F1 10 Tf 82 625 Td [(89) -8800 (0.2)] TJ ET",
    )
}

/// source文字列の結合矩形の中央へ押し込まず、二行二列の編集可能なセルを保つ。
#[test]
fn numeric_tj_values_keep_separate_editable_source_cells() {
    assert_eq!(table_rows(&source()), [["376", "-2.4"], ["89", "0.2"]]);
}

/// 横幅を半分にした原本でも、別の数値欄3と768を一つの整数3768へ連結しない。
#[test]
fn horizontally_scaled_numeric_fields_keep_their_body_word_separator() {
    let input = owned_pdf_content("BT /F1 10 Tf 50 Tz 20 600 Td [(3) -1000 (768)] TJ ET");
    assert_eq!(body_text(&input), "3 768");
}

/// 小さいtrackingは値を分けず、明示空白は一つだけ保ち、添字へは語間を挿入しない。
#[test]
fn numeric_body_spacing_retains_tracking_explicit_spaces_and_scripts() {
    for (content, expected) in [
        (
            "BT /F1 10 Tf 50 Tz 20 600 Td [(3) -100 (768)] TJ ET",
            "3768",
        ),
        (
            "BT /F1 10 Tf 50 Tz 20 600 Td [(3 ) -1000 (768)] TJ ET",
            "3 768",
        ),
        (
            "BT /F1 10 Tf 20 600 Td (10) Tj ET BT /F1 6 Tf 34 604 Td (6) Tj ET",
            "106",
        ),
    ] {
        assert_eq!(body_text(&owned_pdf_content(content)), expected);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/wide_numeric_tj_tables.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 正の水平倍率とemを変えても、原本の異なる3と768を別の値として編集できる。
    #[test]
    fn scaled_numeric_fields_keep_their_logical_separator(size in 5u16..25, scale in 25u16..80, adjustment in 1000u16..1800) {
        let input = owned_pdf_content(&format!("BT /F1 {size} Tf {scale} Tz 20 600 Td [(3) -{adjustment} (768)] TJ ET"));
        prop_assert_eq!(body_text(&input), "3 768");
    }
}

/// Word本文のrun境界を除いて、編集時に見える文字列を公開byte APIから読む。
fn body_text(source: &[u8]) -> String {
    let xml = document_xml(source);
    let mut reader = Reader::from_str(&xml);
    let mut out = String::new();
    let mut text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => text = false,
            Event::Text(value) if text => out.push_str(&value.decode().unwrap()),
            Event::Eof => return out,
            _ => {}
        }
    }
}

/// 半emの桁差を持つ所有数値表でも、公開変換から狭いセルの元の右端anchorを保持する。
#[test]
fn rounded_half_em_numbers_keep_source_right_anchors_in_public_docx() {
    let bytes = owned_pdf_content(
        "1 w 126 600 m 182 600 l S 126 650 m 182 650 l S 126 600 m 126 650 l S 154 600 m 154 650 l S 182 600 m 182 650 l S \
         BT /F1 6.96 Tf 83.333333 Tz 1 0 0 1 142.43892 640 Tm (6.1) Tj ET \
         BT /F1 6.96 Tf 83.333333 Tz 1 0 0 1 170.43892 640 Tm (5.8) Tj ET \
         BT /F1 6.96 Tf 83.333333 Tz 1 0 0 1 142.43892 625 Tm (6.5) Tj ET \
         BT /F1 6.96 Tf 83.333333 Tz 1 0 0 1 170.43892 625 Tm (7.2) Tj ET \
         BT /F1 6.96 Tf 83.333333 Tz 1 0 0 1 138.95892 610 Tm (21.0) Tj ET \
         BT /F1 6.96 Tf 83.333333 Tz 1 0 0 1 166.95892 610 Tm (22.8) Tj ET",
    );
    assert_eq!(table_rows(&bytes), [["6.16.521.0", "5.87.222.8"]]);
    let xml = document_xml(&bytes);
    for text in ["21.0", "22.8"] {
        let cell = xml
            .split("<w:tc>")
            .filter_map(|part| part.split("</w:tc>").next())
            .find(|part| part.contains(&format!(">{text}</w:t>")))
            .unwrap();
        let paragraph = cell
            .split("<w:p ")
            .find(|part| part.contains(&format!(">{text}</w:t>")))
            .unwrap();
        assert!(
            paragraph.contains("<w:jc w:val=\"right\""),
            "{text}: {paragraph}"
        );
    }
}

/// 内容streamから既存と同じobject順・xref形式を生成し、所有対照のbyteを保つ。
fn owned_pdf_content(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".to_string(),
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

/// 公開byte APIのDOCXから、Word表の行とセルの本文を読み戻す。
fn table_rows(source: &[u8]) -> Vec<Vec<String>> {
    let xml = document_xml(source);
    let mut reader = Reader::from_str(&xml);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:tr" => row.clear(),
            Event::Start(tag) if tag.name().as_ref() == b"w:tc" => cell.clear(),
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => text = false,
            Event::Text(value) if text => cell.push_str(&value.decode().unwrap()),
            Event::End(tag) if tag.name().as_ref() == b"w:tc" => row.push(cell.clone()),
            Event::End(tag) if tag.name().as_ref() == b"w:tr" => rows.push(row.clone()),
            Event::Eof => return rows,
            _ => {}
        }
    }
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
