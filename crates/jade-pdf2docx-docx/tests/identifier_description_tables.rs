//! 長い技術コードの説明表と、通常の二段組本文を区別する公開byte APIの契約。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 所有する一ページ原本へ、注記と独立した左右列の表示行を描く。
fn source(count: usize, reverse: bool, article: bool) -> Vec<u8> {
    let mut content = String::from("BT /F1 10 Tf 54 740 Td (Public field notes) Tj ET");
    let mut order = (0..count).collect::<Vec<_>>();
    if reverse {
        order.reverse();
    }
    for index in order {
        let y = 700 - index * 14;
        let left = if article {
            format!("Several ordinary words in sentence {index}")
        } else {
            format!("FIELD_CODE_{index:02}")
        };
        content.push_str(&format!(
            " BT /F1 10 Tf 54 {y} Td ({left}) Tj ET BT /F1 10 Tf 260 {y} Td (Description for original item {index:02}) Tj ET"
        ));
    }
    content.push_str(" BT /F1 10 Tf 54 100 Td (End of field notes) Tj ET");
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

/// 公開変換の本文XMLと、Word表の独立した行・セル文字列を返す。
fn result(source: &[u8]) -> (String, Vec<Vec<String>>) {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-field-table.pdf", source).unwrap();
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
            Event::Eof => break,
            _ => {}
        }
    }
    (xml, rows)
}

/// 多数の反復行を通常の本文と誤認せず、原本のコードと説明の組を一行ずつ保つ。
#[test]
fn dense_identifier_rows_keep_each_original_description_and_surrounding_notes() {
    let expected = (0..30)
        .map(|i| {
            vec![
                format!("FIELD_CODE_{i:02}"),
                format!("Description for original item {i:02}"),
            ]
        })
        .collect::<Vec<_>>();
    for reverse in [false, true] {
        let (xml, rows) = result(&source(30, reverse, false));
        assert_eq!(
            rows, expected,
            "source row ownership must survive dense content and reversed paint order"
        );
        assert!(xml.contains("Public field notes") && xml.contains("End of field notes"));
        assert!(
            !xml.contains("w:val=\"single\""),
            "do not invent visible borders"
        );
    }
}

/// 左右に長い文章がある通常の段組は、コード説明表へ昇格しない。
#[test]
fn ordinary_article_columns_remain_outside_identifier_tables() {
    assert!(result(&source(30, false, true)).1.is_empty());
}
