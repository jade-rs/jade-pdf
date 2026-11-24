//! 原本で一行の中央揃えセルへ既定左右余白を追加し、表示幅を減らさない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};

/// 26pt幅に収まる16ptの二桁を中央に描き、右隣に独立した小さい本文を置く。
fn owned_source() -> Vec<u8> {
    let content = "1 w 72 600 m 198 600 l S 72 650 m 198 650 l S 72 600 m 72 650 l S 98 600 m 98 650 l S 198 600 m 198 650 l S BT /F1 16 Tf 76.104 625 Td (10) Tj ET BT /F1 8 Tf 108 625 Td (Control) Tj ET";
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
    bytes.extend_from_slice(format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    bytes
}

/// 公開byte変換が作るセルの原本fontと中央揃えを保ち、左右余白を明示的にゼロにする。
#[test]
fn source_centered_single_line_keeps_full_cell_width() {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-centered-cell.pdf", &owned_source())
        .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    let start = xml.find("<w:tc>").unwrap();
    let end = start + xml[start..].find("</w:tc>").unwrap();
    let cell = &xml[start..end];
    assert!(cell.contains("w:val=\"center\""), "owned number must be centered: {cell}");
    assert!(cell.contains("w:sz w:val=\"32\""), "source size must be preserved: {cell}");
    assert!(cell.contains("<w:left w:w=\"0\" w:type=\"dxa\""), "source centered line must retain full cell width: {cell}");
    assert!(cell.contains("<w:right w:w=\"0\" w:type=\"dxa\""), "source centered line must retain full cell width: {cell}");
}
