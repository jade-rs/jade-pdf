//! 一・二ページではfooterと確定できない裸の数字を、元の本文位置から消さない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 裸の数字と旧来の装飾付きラベルを、同じ正式ページ構造で作る。
fn source(numbers: &[&str], decorated: bool) -> Vec<u8> {
    let font = 3 + numbers.len() * 2;
    let kids = (0..numbers.len())
        .map(|i| format!("{} 0 R", 3 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count {} /Kids [{kids}] >>", numbers.len()),
    ];
    for (i, number) in numbers.iter().enumerate() {
        let label = if decorated {
            format!("-{number}-")
        } else {
            (*number).to_string()
        };
        let x = 306. - 11.04 * 0.556 * label.len() as f64 / 2.;
        let body = char::from(b'A' + u8::try_from(i).unwrap());
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F {font} 0 R >> >> /Contents {} 0 R >>",4+i*2));
        objects.push(fixture::stream(&format!(
            "BT /F 12 Tf 72 700 Td (Body{body}) Tj ET BT /F 11.04 Tf {x} 52.344 Td ({label}) Tj ET"
        )));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    fixture::pdf(&objects)
}

/// 実際のWord本文とfooter part数を読み、本文に保持した数字をfooter認定と混同しない。
fn content(output: Vec<u8>) -> (String, usize) {
    let mut zip = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let footers = zip
        .file_names()
        .filter(|n| n.starts_with("word/footer") && n.ends_with(".xml"))
        .count();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut in_text = false;
    let mut text = String::new();
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Start(e) if e.name().as_ref() == b"w:t" => in_text = true,
            quick_xml::events::Event::End(e) if e.name().as_ref() == b"w:t" => in_text = false,
            quick_xml::events::Event::Text(t) if in_text => text.push_str(&t.decode().unwrap()),
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    (text, footers)
}

/// 連番を確認するだけのページ数がなくても、原本の数字と先頭ゼロを一度ずつ保持する。
#[test]
fn short_documents_keep_unconfirmed_bare_numbers_in_body() {
    for numbers in [vec!["1"], vec!["1", "2"], vec!["007"]] {
        let output = convert_minimum_pdf_bytes_to_docx_bytes(
            "owned-unconfirmed-short-numbers.pdf",
            &source(&numbers, false),
        )
        .unwrap();
        let (text, footers) = content(output);
        let expected = numbers
            .iter()
            .enumerate()
            .map(|(i, n)| format!("Body{}{n}", char::from(b'A' + u8::try_from(i).unwrap())))
            .collect::<String>();
        assert_eq!(text, expected);
        assert_eq!(footers, 0);
    }
}

/// 装飾された旧ページラベルの既存経路は、裸の数字の保持へ広げない。
#[test]
fn decorated_page_label_keeps_the_existing_route() {
    let output = convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-decorated-number-control.pdf",
        &source(&["1"], true),
    )
    .unwrap();
    assert_eq!(content(output).0, "BodyA");
}

/// native file変換でも元の2数字を保持し、原本のbyteを変えない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_short_document_keeps_both_unconfirmed_numbers() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    let dir = tempfile::tempdir().unwrap();
    let input = source(&["1", "2"], false);
    let path = dir.path().join("owned-source.pdf");
    let output = dir.path().join("owned.docx");
    std::fs::write(&path, &input).unwrap();
    convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
        input_pdf_path: path.clone(),
        output_docx_path: output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    })
    .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), input);
    assert_eq!(
        content(std::fs::read(output).unwrap()),
        ("BodyA1BodyB2".into(), 0)
    );
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 数字の値・桁数と一・二ページの境界を変えても、裸の原文を一度ずつ本文へ残す。
    #[test]
    fn short_document_values_and_page_counts_preserve_source_text(first in 0_u16..=200,count in 1_usize..=2) {
        let numbers=(0..count).map(|i|(u32::from(first)+i as u32).to_string()).collect::<Vec<_>>();
        let borrowed=numbers.iter().map(String::as_str).collect::<Vec<_>>();
        let output=convert_minimum_pdf_bytes_to_docx_bytes("owned-short-number-invariant.pdf",&source(&borrowed,false)).unwrap();
        let expected=numbers.iter().enumerate().map(|(i,n)|format!("Body{}{n}",char::from(b'A'+u8::try_from(i).unwrap()))).collect::<String>();
        prop_assert_eq!(content(output),(expected,0));
    }
}
