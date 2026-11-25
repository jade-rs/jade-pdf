//! 罫線が省略された本文行でも、列をまたぐ原本の行の対応をWordで編集できるように保つ。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 外枠・列罫線・見出し下線だけを持ち、本文の各列へ同じbaselineの値を描く所有原本。
fn source(count: u16, pitch: u16, reverse: bool, synchronized: bool) -> Vec<u8> {
    let bottom = 650 - count * pitch;
    let mut content = format!(
        "1 w 72 {bottom} m 272 {bottom} l S 72 670 m 272 670 l S 72 700 m 272 700 l S 72 {bottom} m 72 700 l S 172 {bottom} m 172 700 l S 272 {bottom} m 272 700 l S BT /F1 10 Tf 82 680 Td (Label) Tj ET BT /F1 10 Tf 182 680 Td (Value) Tj ET"
    );
    let mut order = (0..count).collect::<Vec<_>>();
    if reverse {
        order.reverse();
    }
    for index in order {
        let y = 650 - index * pitch;
        content.push_str(&format!(" BT /F1 10 Tf 82 {y} Td (Label-{index}) Tj ET"));
        if synchronized || index == 0 {
            content.push_str(&format!(" BT /F1 10 Tf 182 {y} Td (Value-{index}) Tj ET"));
        }
    }
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

/// 公開byte APIのDOCXから、Word表の行とセルの本文を読み戻す。
fn table_rows(source: &[u8]) -> Vec<Vec<String>> {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-logical-rows.pdf", source).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
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

/// 文字の集合が一致しても、複数の名称・値を一つのWord行へまとめた出力は合格にしない。
#[test]
fn unruled_body_lines_retain_unambiguous_row_association() {
    assert_eq!(
        table_rows(&source(3, 25, false, true)),
        expected(3),
        "each original label must share a distinct Word row with its own value"
    );
}

/// 同期する反復行を持たない複数行のセルは、他列の一行と推測で分割しない。
#[test]
fn unmatched_multiline_cells_keep_the_original_grid_rows() {
    assert_eq!(table_rows(&source(3, 25, false, false)).len(), 2);
}

/// 所有原本を描画する前に定義した、物理的な上から下への名称・値の対応。
fn expected(count: u16) -> Vec<Vec<String>> {
    std::iter::once(vec!["Label".to_string(), "Value".to_string()])
        .chain((0..count).map(|index| vec![format!("Label-{index}"), format!("Value-{index}")]))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/source_table_logical_rows.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 行数・行間・描画順にかかわらず、各値は対応する名称と同じ一意なWord行に残る。
    #[test]
    fn original_row_association_is_independent_of_paint_order(
        count in 3u16..7,
        pitch in 16u16..30,
        reverse in any::<bool>(),
    ) {
        prop_assert_eq!(table_rows(&source(count, pitch, reverse, true)), expected(count));
    }
}
