//! FontDescriptorの通常weightと、原本の同色fill-and-stroke強調を区別する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use jade_pdf2docx_parser::parse_pdf_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 同じweight400のFontで、細い同色strokeを重ねた文字と通常の文字を描く。
fn source() -> Vec<u8> {
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 300 240] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        page_attribute_pdf::stream("0 g 0 G .24 w BT /F1 12 Tf 1 0 0 1 30 180 Tm 2 Tr (Painted) Tj 0 Tr 1 0 0 1 30 150 Tm (Regular) Tj ET"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>".into(),
        "<< /Type /FontDescriptor /FontName /Courier /FontWeight 400 /Flags 32 /FontBBox [0 -200 600 800] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>".into(),
    ])
}

/// 実際のFontDescriptorは通常体のまま、細い同色の原本強調だけをWordへ渡す。
#[test]
fn source_paint_emphasis_does_not_rewrite_declared_font_weight() {
    let source=source();
    let parsed=parse_pdf_bytes("owned-paint-emphasis.pdf",&source).unwrap();
    for word in ["Painted","Regular"] {
        let element=parsed.text_elements.iter().find(|t|t.text==word).unwrap();
        assert_eq!(element.font_weight,Some(400),"{word}: retain actual descriptor weight");
    }
    let bytes=convert_minimum_pdf_bytes_to_docx_bytes("owned-paint-emphasis.pdf",&source).unwrap();
    let mut archive=zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml=String::new();archive.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    for (word,bold) in [("Painted",true),("Regular",false)] {
        let run=xml.split("<w:r>").find(|part|part.contains(&format!(">{word}</w:t>"))).unwrap().split("</w:r>").next().unwrap();
        let actual=run.contains("<w:b />")||run.contains("<w:b w:val=\"true\"");
        assert_eq!(actual,bold,"{word}: {run}");
    }
}
