//! 原本で一行の中央揃えセルへ既定左右余白を追加し、表示幅を減らさない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};

/// 26pt幅に収まる16ptの二桁を中央に描き、右隣に独立した小さい本文を置く。
fn owned_source() -> Vec<u8> {
    source_with_geometry(72, 26, 16, 4.104)
}

/// 原本gridと文字原点を独立に変え、段落原点の相対量を検証する。
fn source_with_geometry(left: u16, width: u16, size: u16, inset: f32) -> Vec<u8> {
    let middle = left + width;
    let right = middle + 100;
    let x = f32::from(left) + inset;
    let control = middle + 10;
    let content = format!(
        "1 w {left} 600 m {right} 600 l S {left} 650 m {right} 650 l S {left} 600 m {left} 650 l S {middle} 600 m {middle} 650 l S {right} 600 m {right} 650 l S BT /F1 {size} Tf {x} 625 Td (10) Tj ET BT /F1 8 Tf {control} 625 Td (Control) Tj ET"
    );
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

/// 公開byte変換が作るセルに、原本の水平原点を明示し、既定余白を重ねない。
#[test]
fn source_centered_single_line_keeps_full_cell_width() {
    let cell = first_cell(&owned_source());
    assert!(
        cell.contains("w:sz w:val=\"32\""),
        "source size must be preserved: {cell}"
    );
    assert!(
        cell.contains("<w:ind w:left=\"82\""),
        "source single-line horizontal origin must be preserved: {cell}"
    );
    assert_zero_horizontal_margins(&cell);
}

/// 完成したDOCXの最初のセルを読み、複数の原本で同じ製品経路を検証する。
fn first_cell(source: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-centered-cell.pdf", source).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let start = xml.find("<w:tc>").unwrap();
    let end = start + xml[start..].find("</w:tc>").unwrap();
    xml[start..end].to_string()
}

/// 原本の相対indentへ左右の既定余白が加算されないことを検証する。
fn assert_zero_horizontal_margins(cell: &str) {
    assert!(
        cell.contains("<w:left w:w=\"0\" w:type=\"dxa\""),
        "source centered line must retain full cell width: {cell}"
    );
    assert!(
        cell.contains("<w:right w:w=\"0\" w:type=\"dxa\""),
        "source centered line must retain full cell width: {cell}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// gridの移動・列幅・font sizeを変えても、原点の相対量と原本の字面・sizeを保つ。
    #[test]
    fn source_inset_is_invariant_under_grid_translation(
        left in 10_u16..200, width in 30_u16..90, size in 6_u16..18, inset in 1_u16..10,
    ) {
        let cell = first_cell(&source_with_geometry(left, width, size, f32::from(inset)));
        prop_assert!(cell.contains(&format!("<w:ind w:left=\"{}\"", inset * 20)));
        prop_assert!(cell.contains(&format!("w:sz w:val=\"{}\"", size * 2)));
        prop_assert_eq!(cell.matches(">10</w:t>").count(), 1);
        assert_zero_horizontal_margins(&cell);
    }
}
