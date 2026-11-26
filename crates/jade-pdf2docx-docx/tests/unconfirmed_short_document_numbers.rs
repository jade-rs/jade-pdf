//! 一・二ページではfooterと確定できない裸の数字を、元の本文位置から消さない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor,Read};
#[allow(dead_code)]
#[path="../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 裸の数字と旧来の装飾付きラベルを、同じ正式ページ構造で作る。
fn source(numbers:&[&str],decorated:bool)->Vec<u8> {
    let font=3+numbers.len()*2;
    let kids=(0..numbers.len()).map(|i|format!("{} 0 R",3+i*2)).collect::<Vec<_>>().join(" ");
    let mut objects=vec!["<< /Type /Catalog /Pages 2 0 R >>".into(),format!("<< /Type /Pages /Count {} /Kids [{kids}] >>",numbers.len())];
    for (i,number) in numbers.iter().enumerate() {
        let label=if decorated {format!("-{number}-")} else {(*number).to_string()};
        let x=306.-11.04*0.556*label.len() as f64/2.;
        let body=char::from(b'A'+u8::try_from(i).unwrap());
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F {font} 0 R >> >> /Contents {} 0 R >>",4+i*2));
        objects.push(fixture::stream(&format!("BT /F 12 Tf 72 700 Td (Body{body}) Tj ET BT /F 11.04 Tf {x} 52.344 Td ({label}) Tj ET")));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    fixture::pdf(&objects)
}

/// 実際のWord本文とfooter part数を読み、本文に保持した数字をfooter認定と混同しない。
fn content(output:Vec<u8>)->(String,usize) {
    let mut zip=zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let footers=zip.file_names().filter(|n|n.starts_with("word/footer") && n.ends_with(".xml")).count();
    let mut xml=String::new();zip.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut reader=quick_xml::Reader::from_str(&xml);let mut in_text=false;let mut text=String::new();
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Start(e) if e.name().as_ref()==b"w:t"=>in_text=true,
            quick_xml::events::Event::End(e) if e.name().as_ref()==b"w:t"=>in_text=false,
            quick_xml::events::Event::Text(t) if in_text=>text.push_str(&t.decode().unwrap()),
            quick_xml::events::Event::Eof=>break,
            _=>{},
        }
    }
    (text,footers)
}

/// 連番を確認するだけのページ数がなくても、原本の数字と先頭ゼロを一度ずつ保持する。
#[test]
fn short_documents_keep_unconfirmed_bare_numbers_in_body() {
    for numbers in [vec!["1"],vec!["1","2"],vec!["007"]] {
        let output=convert_minimum_pdf_bytes_to_docx_bytes("owned-unconfirmed-short-numbers.pdf",&source(&numbers,false)).unwrap();
        let (text,footers)=content(output);
        let expected=numbers.iter().enumerate().map(|(i,n)|format!("Body{}{n}",char::from(b'A'+u8::try_from(i).unwrap()))).collect::<String>();
        assert_eq!(text,expected);
        assert_eq!(footers,0);
    }
}

/// 装飾された旧ページラベルの既存経路は、裸の数字の保持へ広げない。
#[test]
fn decorated_page_label_keeps_the_existing_route() {
    let output=convert_minimum_pdf_bytes_to_docx_bytes("owned-decorated-number-control.pdf",&source(&["1"],true)).unwrap();
    assert_eq!(content(output).0,"BodyA");
}
