//! TJの明示間隔で離れた原本数値を、Wordの別セルへ割り当てる。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
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
