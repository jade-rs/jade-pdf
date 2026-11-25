//! 同じ原本位置・書式の連続した裸の数字を、検証できたページだけのfooterへ戻す。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use quick_xml::{Reader, events::Event};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

/// 外部assetを使わず、本文と原本下側8%付近の裸のページ番号を持つ所有PDFを作る。
fn source(numbers: &[u32]) -> Vec<u8> {
    let font = 3 + numbers.len() * 2;
    let kids = (0..numbers.len())
        .map(|i| format!("{} 0 R", 3 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", numbers.len()),
    ];
    for (i, number) in numbers.iter().enumerate() {
        let x = 306.0 - 11.04 * 0.556 * number.to_string().len() as f64 / 2.0;
        let body = char::from(b'A' + u8::try_from(i).unwrap());
        let content = format!(
            "BT /F1 12 Tf 72 700 Td (Body{body}) Tj ET BT /F1 11.04 Tf {x:.5} 52.344 Td ({number}) Tj ET"
        );
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font} 0 R >> >> /Contents {} 0 R >>",4+i*2));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    );
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// 生成物のXMLを展開し、ファイル名の並びではなくsection参照順でfooterを読む。
fn package(numbers: &[u32]) -> (String, Vec<String>) {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-bare-footers.pdf", &source(numbers))
        .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let names = zip
        .file_names()
        .filter(|n| n.ends_with(".xml") || n.ends_with(".rels"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut parts = BTreeMap::new();
    for name in names {
        let mut xml = String::new();
        zip.by_name(&name)
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        parts.insert(name, xml);
    }
    let document = parts["word/document.xml"].clone();
    let targets = attributes(&parts["word/_rels/document.xml.rels"], b"Relationship")
        .into_iter()
        .map(|a| (a["Id"].clone(), a["Target"].clone()))
        .collect::<BTreeMap<_, _>>();
    let footers = attributes(&document, b"w:footerReference")
        .iter()
        .map(|a| parts[&format!("word/{}", targets[&a["r:id"]])].clone())
        .collect();
    (document, footers)
}

/// テスト用のASCII属性を読み、偶然本文に現れる数字をsection参照と取り違えない。
fn attributes(xml: &str, name: &[u8]) -> Vec<BTreeMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut found = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) | Event::Empty(t) if t.name().as_ref() == name => found.push(
                t.attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (
                            String::from_utf8(a.key.as_ref().to_vec()).unwrap(),
                            String::from_utf8(a.value.to_vec()).unwrap(),
                        )
                    })
                    .collect(),
            ),
            Event::Eof => return found,
            _ => {}
        }
    }
}

/// 実際に見える文字だけを抽出し、XML属性の番号やsizeを本文と混同しない。
fn text(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut visible = false;
    let mut out = String::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) if t.name().as_ref() == b"w:t" => visible = true,
            Event::End(t) if t.name().as_ref() == b"w:t" => visible = false,
            Event::Text(t) if visible => out.push_str(&t.decode().unwrap()),
            Event::Eof => return out,
            _ => {}
        }
    }
}

/// 消えていた原本の裸の番号を、対応する4つのsectionのfooterへ一度ずつ戻す。
#[test]
fn verified_four_page_bare_sequence_retains_literal_text_style_and_origin() {
    let (document, footers) = package(&[1, 2, 3, 4]);
    assert_eq!(
        footers.iter().map(|f| text(f)).collect::<Vec<_>>(),
        ["1", "2", "3", "4"]
    );
    assert_eq!(text(&document), "BodyABodyBBodyCBodyD");
    for footer in footers {
        assert!(
            attributes(&footer, b"w:sz")
                .iter()
                .any(|a| a["w:val"] == "22")
        );
    }
    assert!(
        attributes(&document, b"w:pgMar")
            .iter()
            .all(|a| a["w:footer"] == "1046")
    );
}

/// 欠番を含む数字を、全ページで確認できた連番footerとは扱わない。
#[test]
fn nonconsecutive_bare_numbers_do_not_create_footer_parts() {
    assert!(package(&[1, 3, 4, 5]).1.is_empty());
}
