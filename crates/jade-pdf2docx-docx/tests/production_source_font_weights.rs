//! 原本descriptorのweightを、font名の推測と区別して公開DOCXへ保持する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};
#[path = "../../jade-pdf2docx-parser/tests/support/source_font_weight.rs"]
mod owned;
/// 実際のdocument.xmlから、本文runと明示されたboldだけを読む。
fn output(weight: Option<u16>, name: &str) -> Vec<(String, Option<bool>)> {
    let entries = weight
        .map(|w| format!("/FontWeight {w}"))
        .unwrap_or_default();
    let pdf = owned::source(&entries, name, None, false);
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-font-weight.pdf", &pdf).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut in_run = false;
    let mut in_text = false;
    let mut text = String::new();
    let mut bold = None;
    let mut rows = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) if t.name().as_ref() == b"w:r" => {
                in_run = true;
                text.clear();
                bold = None;
            }
            Event::Start(t) if in_run && t.name().as_ref() == b"w:t" => in_text = true,
            Event::Empty(t) | Event::Start(t) if in_run && t.name().as_ref() == b"w:b" => {
                bold = Some(
                    t.try_get_attribute(b"w:val")
                        .unwrap()
                        .is_none_or(|v| !matches!(v.value.as_ref(), b"false" | b"0" | b"off")),
                );
            }
            Event::Text(t) if in_text => text.push_str(&t.decode().unwrap()),
            Event::End(t) if t.name().as_ref() == b"w:t" => in_text = false,
            Event::End(t) if t.name().as_ref() == b"w:r" => {
                if !text.is_empty() {
                    rows.push((text.clone(), bold));
                }
                in_run = false;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    rows
}
/// Regularという名前でも、descriptorで確認した700以上を太字として出力する。
#[test]
fn declared_weight_overrides_regular_name() {
    for weight in [1, 400, 500, 699, 700, 828, 1000] {
        assert_eq!(
            output(Some(weight), "Vendor-Regular"),
            vec![("Weight".into(), Some(weight >= 700))]
        );
    }
}
/// Boldという名前より明示400を優先し、欠落時は従来の名前による扱いを保つ。
#[test]
fn explicit_normal_weight_overrides_bold_name_without_changing_missing_behavior() {
    assert_eq!(
        output(Some(400), "Vendor-Bold"),
        vec![("Weight".into(), Some(false))]
    );
    assert_eq!(
        output(None, "Vendor-Bold"),
        vec![("Weight".into(), Some(true))]
    );
}
