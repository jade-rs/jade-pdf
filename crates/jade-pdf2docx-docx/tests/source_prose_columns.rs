//! 独立した散文列を罫線付きの表へ変えず、識別子の表は編集可能なまま保つ。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

/// 外部assetを使わず、散文二列または識別子と説明の二列を持つ所有PDFを作る。
fn source(rows: usize, size: u16, codes: bool) -> (Vec<u8>, String) {
    source_with_columns(rows, size, codes, 2)
}

/// 未検証の三列境界も同じ所有テキストとPDF構築で比較する。
fn source_with_columns(rows: usize, size: u16, codes: bool, columns: usize) -> (Vec<u8>, String) {
    assert!([2, 3].contains(&columns));
    let mut content = "BT /F1 12 Tf 54 740 Td (Owned prose control) Tj ET".to_string();
    let mut expected = "Owned prose control".to_string();
    for row in 0..rows {
        let left = if codes {
            format!("FIELD_CODE_{row:02}")
        } else {
            format!("Left complete sentence number {row:02}")
        };
        let right = if codes {
            format!("Original description for field {row:02}")
        } else {
            format!("Right complete sentence number {row:02}")
        };
        let y = 700 - row * if codes { 14 } else { 20 };
        let third = format!("Third complete sentence number {row:02}");
        let xs: &[u16] = if columns == 2 {
            &[54, 350]
        } else {
            &[54, 250, 446]
        };
        for (index, x) in xs.iter().enumerate() {
            let text = [&left, &right, &third][index];
            content.push_str(&format!(" BT /F1 {size} Tf {x} {y} Td ({text}) Tj ET"));
            expected.push_str(text);
        }
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
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
    (bytes, expected)
}

/// 公開byte APIの実際のWord本文XMLを取り出す。
fn document(source: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-prose-columns.pdf", source).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// XMLの要素数・実段組の指定・可視本文を、属性の数字と区別して調べる。
fn properties(xml: &str) -> (usize, usize, bool, String) {
    let mut reader = Reader::from_str(xml);
    let (mut tables, mut rows, mut columns, mut visible) = (0, 0, false, false);
    let mut text = String::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) | Event::Empty(t) => match t.name().as_ref() {
                b"w:tbl" => tables += 1,
                b"w:tr" => rows += 1,
                b"w:t" => visible = true,
                b"w:cols" => {
                    columns |= t.attributes().any(|a| {
                        let a = a.unwrap();
                        a.key.as_ref() == b"w:num" && a.value.as_ref() == b"2"
                    });
                }
                _ => {}
            },
            Event::End(t) if t.name().as_ref() == b"w:t" => visible = false,
            Event::Text(t) if visible => text.push_str(&t.decode().unwrap()),
            Event::Eof => return (tables, rows, columns, text),
            _ => {}
        }
    }
}

/// 段組による表示順の変化と、文字自体の欠落・追加を混同しない。
fn inventory(text: &str) -> BTreeMap<char, usize> {
    let mut result = BTreeMap::new();
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        *result.entry(c).or_default() += 1;
    }
    result
}

/// 原本に罫線のない独立した散文二列は、表を発明せず実際の二段組で保持する。
#[test]
fn ordinary_prose_columns_do_not_create_ruled_tables() {
    let (source, expected) = source(12, 10, false);
    let (tables, _, columns, text) = properties(&document(&source));
    assert_eq!(tables, 0, "independent prose must not become ruled tables");
    assert!(columns, "preserve the actual two-column body flow");
    assert_eq!(inventory(&text), inventory(&expected));
}

/// 識別子と説明を持つ原本を散文と誤判定せず、既存の編集可能な表を維持する。
#[test]
fn code_description_rows_keep_their_editable_table() {
    let (source, expected) = source(30, 10, true);
    let (tables, rows, _, text) = properties(&document(&source));
    assert_eq!((tables, rows), (1, 30));
    assert_eq!(inventory(&text), inventory(&expected));
}

/// 三列を未検証の二段組経路へ入れない。旧表推定の外観の正しさは承認しない。
#[test]
fn three_column_prose_keeps_unqualified_legacy_fallback() {
    let (source, expected) = source_with_columns(10, 8, false, 3);
    let (tables, rows, columns, text) = properties(&document(&source));
    assert_eq!(
        (tables, rows),
        (1, 10),
        "retain the existing three-column fallback"
    );
    assert!(!columns, "do not reinterpret three source columns as two");
    assert_eq!(inventory(&text), inventory(&expected));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/source_prose_columns.proptest-regressions"))),
        ..ProptestConfig::default()
    })]

    /// 行数とfont sizeが変わっても、散文は実段組のまま全可視文字を保持する。
    #[test]
    fn prose_columns_keep_structure_and_text_at_source_sizes(
        rows in 8usize..=18,
        size in 8u16..=12,
    ) {
        let (source,expected)=source(rows,size,false);
        let (tables,_,columns,text)=properties(&document(&source));
        prop_assert_eq!(tables,0);
        prop_assert!(columns);
        prop_assert_eq!(inventory(&text),inventory(&expected));
    }
}
