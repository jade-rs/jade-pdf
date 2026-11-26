//! 原本位置付きセルにも、既存のMS-PGothic代替fontの字幅補正を渡す。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 正式な原本gridへ一行・複数行・収まらない二行を配置し、表外の対照も残す。
fn source(mode: usize, font: &str) -> Vec<u8> {
    let (first, second) = match mode { 0 => (625, None), 1 => (628, Some(608)), _ => (624, Some(620)) };
    let mut content=format!("1 w 72 600 m 272 600 l S 72 650 m 272 650 l S 72 600 m 72 650 l S 172 600 m 172 650 l S 272 600 m 272 650 l S BT /F 10 Tf 82 {first} Td (Alpha) Tj ET BT /F 10 Tf 182 625 Td (Control) Tj ET BT /F 10 Tf 72 710 Td (Outside) Tj ET ");
    if let Some(y)=second {content.push_str(&format!("BT /F 10 Tf 82 {y} Td (Omega) Tj ET "));}
    let widths=std::iter::repeat_n("600",95).collect::<Vec<_>>().join(" ");
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] /FontDescriptor 6 0 R >>"),
        format!("<< /Type /FontDescriptor /FontName /{font} /FontBBox [0 -200 1000 800] /Flags 32 /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 /FontWeight 400 >>"),
    ])
}

/// 製品の本文XMLを読み、文字を持つrunの属性だけを検査する。
fn xml(output: Vec<u8>) -> String {
    let mut zip=zip::ZipArchive::new(Cursor::new(output)).unwrap();let mut text=String::new();
    zip.by_name("word/document.xml").unwrap().read_to_string(&mut text).unwrap();text
}

/// 一意なラベルのrunを切り出し、別セルや表全体のfont指定と混同しない。
fn run<'a>(xml: &'a str,label: &str)-> &'a str {
    let position=xml.find(&format!("{label}</w:t>")).unwrap();
    let start=xml[..position].rfind("<w:r>").unwrap();let end=position+xml[position..].find("</w:r>").unwrap()+"</w:r>".len();&xml[start..end]
}

/// 複数行の原本位置付きセルは、既存の代替familyと92%の字幅を同時に保持する。
#[test]
fn bounded_multiline_cells_keep_the_existing_ms_pgothic_fallback_width() {
    let output=xml(convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-fallback-width.pdf",&source(1,"AAAAAA+MS-PGothic")).unwrap());
    assert_eq!(output.matches("<w:tbl>").count(),1);
    for label in ["Alpha","Omega","Control"] {
        let r=run(&output,label);assert!(r.contains("w:ascii=\"Arial Unicode MS\""));
        assert!(r.contains("<w:w w:val=\"92\""),"missing existing fallback width for {label}: {r}");
    }
    assert!(!run(&output,"Outside").contains("<w:w "),"body route keeps its current width");
}
