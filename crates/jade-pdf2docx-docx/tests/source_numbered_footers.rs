//! 連番の装飾付きページ番号を本文で改ページさせず、原文と書式を各source pageのfooterへ保持する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// Standard14の実em dashをWinAnsiのoctalで描き、外部fontなしで複数ページの所有PDFを作る。
fn source(numbers: &[u32], size: u16, y: u16, shift: i16) -> Vec<u8> {
    let font_id = 3 + numbers.len() * 2;
    let kids = (0..numbers.len())
        .map(|i| format!("{} 0 R", 3 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", numbers.len()),
    ];
    for (i, number) in numbers.iter().enumerate() {
        let content_id = 4 + i * 2;
        let width = f64::from(size) * (2. + 0.278 * 2. + number.to_string().len() as f64 * 0.556);
        let x = 306. + f64::from(shift) + if i % 2 == 0 { 0. } else { 7. } - width / 2.;
        let content = format!(
            "BT /F1 12 Tf 72 700 Td (Body{i}) Tj ET BT /F1 {size} Tf {x:.5} {y} Td (\\227 {number} \\227) Tj ET"
        );
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>"));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    // 非ASCIIのfont fallbackとfooter判定を混同せず、描画する全codeのUnicodeを明示する。
    objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode {} 0 R >>",font_id+1));
    let mut codes = vec![0x20u8, b'B', b'o', b'd', b'y', 0x97];
    codes.extend(b'0'..=b'9');
    codes.sort();
    let mappings = codes
        .iter()
        .map(|c| {
            format!(
                "<{c:02X}> <{:04X}>",
                if *c == 0x97 { 0x2014u16 } else { u16::from(*c) }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /OwnedFooter def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange {} beginbfchar\n{mappings}\nendbfchar endcmap CMapName currentdict /CMap defineresource pop end end",
        codes.len()
    );
    objects.push(format!(
        "<< /Length {} >>\nstream\n{cmap}\nendstream",
        cmap.len()
    ));
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

/// Wordのvisible textだけを読み、書式・属性値の数字を本文と混同しない。
fn texts(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut in_text = false;
    let mut out = String::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) if t.name().as_ref() == b"w:t" => in_text = true,
            Event::End(t) if t.name().as_ref() == b"w:t" => in_text = false,
            Event::Text(t) if in_text => out.push_str(&t.decode().unwrap()),
            Event::Eof => return out,
            _ => {}
        }
    }
}

/// 公開byte APIが出力した本文とすべてのfooter partを取り出す。
fn package(numbers: &[u32], size: u16, y: u16, shift: i16) -> (String, Vec<String>) {
    let input = source(numbers, size, y, shift);
    let parsed = jade_pdf2docx_parser::parse_pdf_bytes(
        std::path::Path::new("owned-numbered-footers.pdf"),
        &input,
    )
    .unwrap();
    let original = parsed
        .text_elements
        .iter()
        .map(|t| t.text.as_str())
        .collect::<String>();
    for number in numbers {
        assert!(
            original.contains(&format!("— {number} —")),
            "owned source footer must have verified literal Unicode before DOCX: {original:?}"
        );
    }
    let bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-numbered-footers.pdf", &input).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut document = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut document)
        .unwrap();
    let names = zip
        .file_names()
        .filter(|n| n.starts_with("word/footer") && n.ends_with(".xml"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let footers = names
        .into_iter()
        .map(|n| {
            let mut xml = String::new();
            zip.by_name(&n).unwrap().read_to_string(&mut xml).unwrap();
            xml
        })
        .collect();
    (document, footers)
}

/// 本文から消えたページ番号を、同じ文字列・sizeでfooterへ一度ずつ保持し、参照も持たせる。
fn assert_numbered_footers(numbers: &[u32], size: u16, y: u16, shift: i16) {
    let (document, footers) = package(numbers, size, y, shift);
    assert_eq!(
        footers.len(),
        numbers.len(),
        "per-source page numbers must move to corresponding footer parts; actual body={:?}",
        texts(&document)
    );
    assert_eq!(
        document.matches("<w:footerReference ").count(),
        numbers.len()
    );
    let body = texts(&document);
    let all = footers.iter().map(|f| texts(f)).collect::<String>();
    for (i, number) in numbers.iter().enumerate() {
        let label = format!("— {number} —");
        assert!(!body.contains(&label));
        assert_eq!(all.matches(&label).count(), 1);
        assert_eq!(body.matches(&format!("Body{i}")).count(), 1);
    }
    assert!(
        footers
            .iter()
            .all(|f| f.contains(&format!("w:sz w:val=\"{}\"", size * 2)))
    );
}

/// 本文とは別のfooter partで、装飾文字を削除せず15・16のsource番号を保持する。
#[test]
fn consecutive_decorated_source_page_numbers_become_footers_without_text_loss() {
    assert_numbered_footers(&[15, 16], 10, 20, 0);
}

/// 連番の根拠がない数値や本文領域の行は、そのまま本文へ残す。
#[test]
fn unrelated_decorated_numbers_remain_in_body() {
    for (numbers, y) in [(vec![15], 20), (vec![15, 17], 20), (vec![15, 16], 100)] {
        let (document, footers) = package(&numbers, 10, y, 0);
        assert!(footers.is_empty());
        for number in numbers {
            assert!(texts(&document).contains(&format!("— {number} —")));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {cases:24,failure_persistence:Some(Box::new(FileFailurePersistence::Direct("tests/source_numbered_footers.proptest-regressions"))),..ProptestConfig::default()})]
    /// sourceの開始番号・桁数・書体サイズ・左右の綴じ余白が変わっても、全番号と文字列を一度ずつ保持する。
    #[test]
    fn source_footer_numbers_survive_counter_and_geometry_changes(start in 1u32..10000,count in 2usize..5,size in 8u16..15,y in 10u16..30,shift in -20i16..21) {
        let numbers=(0..count).map(|i|start+i as u32).collect::<Vec<_>>();assert_numbered_footers(&numbers,size,y,shift);
    }
}
