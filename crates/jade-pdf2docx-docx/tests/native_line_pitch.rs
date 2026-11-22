use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 同じ書体・字号で4行を置き、PDFの行高と空白を独立に宣言する。
fn owned_pdf(size: u32, pitch: u32) -> Vec<u8> {
    let content = (0..4)
        .map(|i| format!("BT /F1 {size} Tf 72 {} Td (Line {}) Tj ET", 740 - i * pitch, i + 1))
        .collect::<Vec<_>>()
        .join(" ");
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
    bytes.extend_from_slice(format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    bytes
}

/// 本文段落の行高と段落前空白を、属性順に依存せず読む。
fn paragraph_spacing(size: u32, pitch: u32) -> Vec<std::collections::BTreeMap<String, String>> {
    let docx = convert_minimum_pdf_bytes_to_docx_bytes("owned-pitch.pdf", &owned_pdf(size, pitch)).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(docx)).unwrap();
    let mut xml = String::new();
    archive.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut rows = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(ref e) if e.name().as_ref() == b"w:p" => rows.push(std::collections::BTreeMap::new()),
            Event::Empty(ref e) if e.name().as_ref() == b"w:spacing" => {
                let row = rows.last_mut().unwrap();
                for attribute in e.attributes() {
                    let attribute = attribute.unwrap();
                    row.insert(String::from_utf8(attribute.key.as_ref().to_vec()).unwrap(), String::from_utf8(attribute.value.to_vec()).unwrap());
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    rows
}

/// 自動行高の1.35pt差が各行へ累積した実描画の失敗を、OOXML境界でも防ぐ。
#[test]
fn authored_twenty_point_pitch_uses_explicit_line_box() {
    let rows = paragraph_spacing(10, 20);
    assert_eq!(rows.len(), 4);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.get("w:lineRule").map(String::as_str), Some("exact"), "paragraph {index}: {row:?}");
        assert_eq!(row.get("w:line").map(String::as_str), Some("200"));
        assert_eq!(row.get("w:after").map(String::as_str), Some("0"));
        if index > 0 {
            assert_eq!(row.get("w:before").map(String::as_str), Some("200"));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// 任意の字号と原本の行間隔でも、行boxと空白の和が元のpitchに戻る。
    #[test]
    fn line_box_and_blank_gap_reconstruct_source_pitch(size in 6u32..37, blank in 4u32..33) {
        let pitch = size + blank;
        let rows = paragraph_spacing(size, pitch);
        prop_assert_eq!(rows.len(), 4);
        for row in rows.iter().skip(1) {
            prop_assert_eq!(row.get("w:lineRule").map(String::as_str), Some("exact"));
            let line = row.get("w:line").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
            let before = row.get("w:before").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
            prop_assert_eq!(line, size * 20);
            prop_assert_eq!(line + before, pitch * 20);
        }
    }
}
