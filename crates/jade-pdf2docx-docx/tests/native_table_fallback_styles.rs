//! 行配置がfallbackしても、原本の各runの書体・weight・色を一つの既定runへ潰さない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 30ptセルへ重なる二行を描き、異なる原本fontと色を持つ所有PDFを作る。
fn source() -> Vec<u8> {
    let content = "1 w 72 670 m 272 670 l S 72 700 m 272 700 l S 72 670 m 72 700 l S 172 670 m 172 700 l S 272 670 m 272 700 l S 1 0 0 rg BT /F1 10 Tf 82 680 Td (Alpha) Tj ET 0 0 1 rg BT /F2 14 Tf 92 674 Td (Beta) Tj ET 0 g BT /F3 10 Tf 182 680 Td (Control) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 8\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 8 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 製品が書いた本文XMLから、ラベルに対応するrunだけを読む。
fn labelled_run<'a>(xml: &'a str, label: &str) -> &'a str {
    let position = xml.find(&format!("{label}</w:t>")).unwrap();
    let start = xml[..position].rfind("<w:r>").unwrap();
    let end = position + xml[position..].find("</w:r>").unwrap() + "</w:r>".len();
    &xml[start..end]
}

/// Wordの明示的な通常体解除を、文字列中のbCsや属性順と取り違えず読む。
fn bold(run: &str) -> Option<bool> {
    let mut reader = Reader::from_str(run);
    loop {
        match reader.read_event().unwrap() {
            Event::Empty(tag) | Event::Start(tag) if tag.name().as_ref() == b"w:b" => {
                return Some(!tag.attributes().any(|a| {
                    let a = a.unwrap();
                    a.key.as_ref() == b"w:val" && matches!(a.value.as_ref(), b"false" | b"0")
                }));
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// 一段落へ戻るセルでも、太字の赤いArialと通常体の青いCourierを別runのまま保持する。
#[test]
fn overfull_legacy_flow_keeps_font_weight_family_and_color_per_run() {
    let bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-overfull-styles.pdf", &source()).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let alpha = labelled_run(&xml, "Alpha");
    let beta = labelled_run(&xml, "Beta");
    assert!(alpha.contains("w:ascii=\"Arial\""));
    assert!(beta.contains("w:ascii=\"Courier New\""));
    assert_eq!(bold(alpha), Some(true));
    assert_eq!(bold(beta), Some(false));
    assert!(alpha.to_ascii_uppercase().contains("FF0000"));
    assert!(beta.to_ascii_uppercase().contains("0000FF"));
}
