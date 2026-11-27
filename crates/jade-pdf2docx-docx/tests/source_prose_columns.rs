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
    (pdf(&content, "612 792", "Helvetica"), expected)
}

/// 用紙サイズと標準fontを指定して、所有の内容を一ページPDFにする。
fn pdf(content: &str, size: &str, font: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {size}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} /Encoding /WinAnsiEncoding >>"),
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
    bytes
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

/// 見出し・末尾注記の間に、値を含む独立した二段を配置する。
fn short_columns(rules: &str) -> Vec<u8> {
    let mut content = format!("{rules} BT /F1 10 Tf 20 240 Td (Heading above both columns) Tj ET");
    for (row, y) in [180, 168, 156, 144, 132, 120].into_iter().enumerate() {
        for (x, value) in [
            (20, format!("Left{row}")),
            (100, format!("{row}11")),
            (160, format!("Right{row}")),
            (250, format!("{row}22")),
        ] {
            content.push_str(&format!(" BT /F1 10 Tf {x} {y} Td ({value}) Tj ET"));
        }
    }
    content.push_str(" BT /F1 10 Tf 20 40 Td (Closing note across columns) Tj ET");
    pdf(&content, "300 260", "Courier")
}

/// 部分的な縦罫線の両側を独立した実段にし、本文の読順と全幅行を保つ。
#[test]
fn partial_height_rule_preserves_the_two_column_reading_order() {
    let xml = document(&short_columns("0.5 w 150 100 m 150 195 l S"));
    let (tables, _, columns, text) = properties(&xml);
    assert_eq!(tables, 0, "a standalone divider must not form a table");
    assert!(columns, "the editable section must have two columns");
    assert!(
        xml.contains(r#"w:sep="1""#),
        "the source divider must stay visible"
    );
    assert_eq!(xml.matches("<w:tab />").count(), 12);
    for position in [1600, 1800] {
        assert_eq!(
            xml.matches(&format!(r#"w:pos="{position}""#)).count(),
            6,
            "values must stay at their source x coordinates"
        );
    }
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut expected = String::from("Headingabovebothcolumns");
    for (side, value) in [("Left", "11"), ("Right", "22")] {
        for row in 0..6 {
            expected.push_str(&format!("{side}{row}{row}{value}"));
        }
    }
    expected.push_str("Closingnoteacrosscolumns");
    assert_eq!(compact, expected);
}

/// 一行だけの縦線と横罫線付きの格子では、既存の編集可能な表を保つ。
#[test]
fn short_divider_or_table_rules_keep_the_existing_table() {
    for rules in [
        "",
        "0.5 w 150 174 m 150 185 l S",
        "0.5 w 150 100 m 150 195 l S 18 185 m 280 185 l S",
    ] {
        let (tables, rows, columns, _) = properties(&document(&short_columns(rules)));
        assert_eq!((tables, rows, columns), (1, 6, false), "{rules}");
    }
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

/// 短い識別子列を無視して、残り二列の説明だけを散文の二段組と誤認しない。
#[test]
fn three_column_code_and_prose_rows_keep_their_editable_table() {
    let (source, expected) = source_with_columns(10, 8, true, 3);
    let (tables, rows, columns, text) = properties(&document(&source));
    assert_eq!((tables, rows), (1, 10));
    assert!(!columns);
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

    /// 行数・文字幅・識別子の有無が変わっても、三列を新しい二列経路へ入れない。
    #[test]
    fn three_column_fallback_is_not_promoted_to_two_columns(
        rows in 8usize..=18,
        size in 8u16..=10,
        codes in any::<bool>(),
    ) {
        let (source,expected)=source_with_columns(rows,size,codes,3);
        let (tables,actual_rows,columns,text)=properties(&document(&source));
        prop_assert_eq!((tables,actual_rows),(1,rows));
        prop_assert!(!columns);
        prop_assert_eq!(inventory(&text),inventory(&expected));
    }
}
