//! 同じ原本位置・書式の連続した裸の数字を、検証できたページだけのfooterへ戻す。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

/// 外部assetを使わず、本文と原本下側8%付近の裸のページ番号を持つ所有PDFを作る。
fn source(numbers: &[u32]) -> Vec<u8> {
    source_at(numbers, 11.04, 52.344, Deviation::None)
}

/// ページごとの原点・書式の食い違いや、別の数字・背景との競合を作る所有対照。
#[derive(Clone, Copy)]
enum Deviation {
    None,
    Font,
    Horizontal,
    Vertical,
    Duplicate,
    Background,
    InteriorSpace,
}

/// 原本の位置・サイズを変え、指定した一ページだけに拒否すべき不一致を与える。
fn source_at(numbers: &[u32], size: f64, y: f64, deviation: Deviation) -> Vec<u8> {
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
        let size = if i == 1 && matches!(deviation, Deviation::Font) {
            size + 2.0
        } else {
            size
        };
        let y = if i == 1 && matches!(deviation, Deviation::Vertical) {
            y + 6.0
        } else {
            y
        };
        let x = 306.0 - size * 0.556 * number.to_string().len() as f64 / 2.0
            + if i == 1 && matches!(deviation, Deviation::Horizontal) {
                5.0
            } else {
                0.0
            };
        let body = char::from(b'A' + u8::try_from(i).unwrap());
        let label = if i == 1 && matches!(deviation, Deviation::InteriorSpace) {
            number
                .to_string()
                .chars()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            number.to_string()
        };
        let mut content = format!(
            "BT /F1 12 Tf 72 700 Td (Body{body}) Tj ET BT /F1 {size} Tf {x:.5} {y} Td ({label}) Tj ET"
        );
        if i == 1 && matches!(deviation, Deviation::Duplicate) {
            content.push_str(&format!(
                " BT /F1 {size} Tf {} {} Td (99) Tj ET",
                306.0 - size * 0.556,
                y + 20.0
            ));
        }
        if i == 1 && matches!(deviation, Deviation::Background) {
            content.insert_str(
                0,
                &format!("0.8 g 290 {} 32 {} re f 0 g ", y - 2.0, size + 4.0),
            );
        }
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
    package_bytes(&source(numbers))
}

/// 同じXML検査を、位置・書式の差を持つ所有対照にも適用する。
fn package_bytes(source: &[u8]) -> (String, Vec<String>) {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-bare-footers.pdf", source).unwrap();
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

/// 二ページだけでは偶然の一致を除けないため、新しい裸の番号の経路へ昇格しない。
#[test]
fn two_bare_numbers_are_not_enough_for_a_verified_sequence() {
    assert!(package(&[1, 2]).1.is_empty());
}

/// 書式・原点・別候補・背景・内部空白のある数字を、原文を変えてfooterへ押し込まない。
#[test]
fn conflicting_source_style_geometry_or_candidates_do_not_create_footers() {
    for deviation in [
        Deviation::Font,
        Deviation::Horizontal,
        Deviation::Vertical,
        Deviation::Duplicate,
        Deviation::Background,
        Deviation::InteriorSpace,
    ] {
        assert!(
            package_bytes(&source_at(&[10, 11, 12, 13], 11.04, 52.344, deviation))
                .1
                .is_empty()
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/source_bare_page_footers.proptest-regressions"))),
        ..ProptestConfig::default()
    })]

    /// 桁数・開始番号・ページ数・em高と原点が変わっても、全原本ラベルを一度ずつ保持する。
    #[test]
    fn verified_bare_sequence_is_covariant_with_source_number_size_and_origin(
        first in 1u32..=50,
        count in 3usize..=6,
        size in 8u16..=12,
        y in 20u16..=80,
    ) {
        let numbers=(0..count).map(|i|first+i as u32).collect::<Vec<_>>();
        let (document,footers)=package_bytes(&source_at(&numbers,f64::from(size),f64::from(y),Deviation::None));
        prop_assert_eq!(footers.iter().map(|f|text(f)).collect::<Vec<_>>(),numbers.iter().map(u32::to_string).collect::<Vec<_>>());
        let expected=(0..count).map(|i|format!("Body{}",char::from(b'A'+i as u8))).collect::<String>();
        prop_assert_eq!(text(&document),expected);
        let margins=attributes(&document,b"w:pgMar");
        prop_assert!(margins.iter().all(|a|a["w:footer"]==(u32::from(y)*20).to_string()));
    }
}
