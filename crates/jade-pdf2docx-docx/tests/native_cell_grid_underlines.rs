//! 復元できる通常セルでも、原本gridの境界と独立した本文下線を取り違えない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};

/// セル下辺の近くに文字を置き、必要なら別の短い本文下線も描く所有PDFを作る。
fn source(multiline: bool, independent: bool, overhang: bool) -> Vec<u8> {
    let y = if multiline { 628 } else { 602 };
    let (x0, x1) = if overhang { (71.4, 272.6) } else { (72., 272.) };
    let mut content = format!(
        "1 w {x0} 600 m {x1} 600 l S {x0} 650 m {x1} 650 l S 72 600 m 72 650 l S 172 600 m 172 650 l S 272 600 m 272 650 l S BT /F1 10 Tf 82 {y} Td (Alpha) Tj ET BT /F1 10 Tf 182 625 Td (Control) Tj ET "
    );
    if multiline {
        content.push_str("BT /F1 10 Tf 82 602 Td (Omega) Tj ET ");
    }
    if independent {
        content.push_str(&format!("82 {} m 108 {} l S ", y - 1, y - 1));
    }
    let objects=[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
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

/// 公開byte APIが返す先頭セルだけを読み、表のborder指定をrunの下線と混同しない。
fn cell(multiline: bool, independent: bool) -> String {
    cell_with_overhang(multiline, independent, false)
}

/// 縦辺と横辺の端点が別々に量子化されるケースも、同じ公開製品経路で読む。
fn cell_with_overhang(multiline: bool, independent: bool, overhang: bool) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-bounded-cell-grid-underline.pdf",
        &source(multiline, independent, overhang),
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let begin = xml.find("<w:tc>").unwrap();
    let end = begin + xml[begin..].find("</w:tc>").unwrap();
    xml[begin..end].to_string()
}

/// ラベル本文を持つ一つのrunだけを抜き出し、罫線や別runの書式から判定しない。
fn run<'a>(xml: &'a str, label: &str) -> &'a str {
    let position = xml.find(&format!(">{label}</w:t>")).unwrap();
    let begin = xml[..position].rfind("<w:r>").unwrap();
    let end = position + xml[position..].find("</w:r>").unwrap() + "</w:r>".len();
    &xml[begin..end]
}

/// 一行・複数行の通常配置に収まる文字でも、下辺と近いだけで本文へ下線を追加しない。
#[test]
fn bounded_native_cells_do_not_inherit_grid_borders_as_text_underlines() {
    for multiline in [false, true] {
        let xml = cell(multiline, false);
        assert_eq!(xml.matches("<w:p ").count(), if multiline { 2 } else { 1 });
        assert!(
            !xml.contains("<w:u "),
            "verified grid edges must not underline bounded source text: {xml}"
        );
    }
}

/// grid端点とは違う短い実線は、本来の対象runだけの下線として残す。
#[test]
fn bounded_native_cells_keep_independent_text_underlines() {
    for multiline in [false, true] {
        let xml = cell(multiline, true);
        assert!(run(&xml, "Alpha").contains("<w:u "));
        if multiline {
            assert!(!run(&xml, "Omega").contains("<w:u "));
        }
    }
}

/// 横辺が縦辺からわずかに延び、各端点の丸め後に1pt違っても、高さが同じgrid辺を下線にしない。
#[test]
fn independently_quantized_grid_endpoints_do_not_underline_text() {
    for multiline in [false, true] {
        for independent in [false, true] {
            let xml=cell_with_overhang(multiline, independent, true);
            assert_eq!(run(&xml,"Alpha").contains("<w:u "),independent,"separately rounded grid endpoints must not become text underlines");
            if multiline {assert!(!run(&xml,"Omega").contains("<w:u "));}
        }
    }
}
