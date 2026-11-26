//! 表の外枠を直前の本文見出しの下線にせず、独立した下線を区別する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 見出しの4pt下にある正式な二列の表と、任意の独立下線を作る。
fn source(independent: bool) -> Vec<u8> {
    let underline=if independent {"82 652 m 124 652 l S"} else {""};
    let content=format!("1 w 72 600 m 272 600 l S 72 650 m 272 650 l S 72 600 m 72 650 l S 172 600 m 172 650 l S 272 600 m 272 650 l S {underline} BT /F 10 Tf 82 654 Td (Heading) Tj ET BT /F 10 Tf 82 625 Td (Alpha) Tj ET BT /F 10 Tf 182 625 Td (Control) Tj ET");
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 製品の本文XMLだけを返す。
fn xml(output: Vec<u8>) -> String {
    let mut z=zip::ZipArchive::new(Cursor::new(output)).unwrap();let mut s=String::new();
    z.by_name("word/document.xml").unwrap().read_to_string(&mut s).unwrap();s
}

/// 対象ラベルのrunだけを検査し、table borderと下線属性を混同しない。
fn run<'a>(xml: &'a str,label: &str)-> &'a str {
    let p=xml.find(&format!("{label}</w:t>")).unwrap();let start=xml[..p].rfind("<w:r>").unwrap();let end=p+xml[p..].find("</w:r>").unwrap()+"</w:r>".len();&xml[start..end]
}

/// 表の完全な上枠は、表外の見出しへ下線を新設する根拠にしない。
#[test]
fn a_complete_source_table_top_rule_does_not_underline_the_body_heading() {
    let output=xml(convert_minimum_pdf_bytes_to_docx_bytes("owned-body-heading-table-rule.pdf",&source(false)).unwrap());
    assert_eq!(output.matches("<w:tbl>").count(),1);
    assert!(!run(&output,"Heading").contains("<w:u "),"table frame is not a body underline: {}",run(&output,"Heading"));
}
