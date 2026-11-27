//! 罫線のない原本の会計符号を、金額と同じ編集可能なWordセルに残す。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};

#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 同じ金額列に正・負の額を描き、必要なら符号専用の見出しも置く。
fn source(right_aligned: bool, sign_header: bool) -> Vec<u8> {
    let mut content = String::new();
    if sign_header {
        content.push_str("BT /F1 10 Tf 20 181 Td (Item) Tj ET BT /F1 10 Tf 120 181 Td (Flag) Tj ET BT /F1 10 Tf 180 181 Td (Amount) Tj ET ");
    }
    for (y, label, sign, amount) in [
        (160, "Deposits", "-", "2,900,000"),
        (146, "Refunds", "", "2,900,000"),
        (132, "Securities", "-", "2,000,000"),
        (118, "Interest", "", "21,527"),
    ] {
        let x = if right_aligned {
            234 - 6 * amount.len()
        } else {
            180
        };
        content.push_str(&format!("BT /F1 10 Tf 20 {y} Td ({label}) Tj ET "));
        if !sign.is_empty() {
            content.push_str(&format!("BT /F1 10 Tf 120 {y} Td ({sign}) Tj ET "));
        }
        content.push_str(&format!("BT /F1 10 Tf {x} {y} Td ({amount}) Tj ET "));
    }
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".into(),
    ])
}

/// 公開変換で書かれた編集用の本文XMLを取り出す。
fn document(pdf: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-accounting.pdf", pdf).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut doc = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut doc)
        .unwrap();
    doc
}

/// 符号・金額を同じセルに残し、左右いずれの金額揃えでも元の組を保持する。
#[test]
fn detached_minus_signs_stay_in_the_editable_amount_cell() {
    for right in [false, true] {
        let doc = document(&source(right, false));
        assert_eq!(doc.matches("<w:tbl>").count(), 1);
        assert_eq!(doc.matches("<w:tr>").count(), 4);
        for (i, row) in doc.split("<w:tr>").skip(1).enumerate() {
            let cells: Vec<_> = row
                .split("</w:tr>")
                .next()
                .unwrap()
                .split("<w:tc>")
                .skip(1)
                .collect();
            assert_eq!(cells.len(), 2);
            assert_eq!(cells[1].contains(">-</w:t>"), i % 2 == 0);
            assert!(cells[1].contains(if i == 2 {
                "2,000,000"
            } else if i == 3 {
                "21,527"
            } else {
                "2,900,000"
            }));
            if i % 2 == 0 {
                assert!(
                    cells[1].contains("<w:tab />"),
                    "the source sign-to-amount gap uses a tab"
                );
                assert!(cells[1].contains(r#"w:pos="1200""#));
            }
            if i == 3 {
                let indent = if right { 1560 } else { 1200 };
                assert!(cells[1].contains(&format!(r#"w:left="{indent}""#)));
            }
        }
    }
}

/// 符号の位置に見出しがある原本は、独立した列として扱う。
#[test]
fn an_explicit_flag_column_is_not_reinterpreted_as_an_amount_sign() {
    let doc = document(&source(false, true));
    assert_eq!(doc.matches("<w:tbl>").count(), 1);
    let header = doc
        .split("<w:tr>")
        .nth(1)
        .unwrap()
        .split("</w:tr>")
        .next()
        .unwrap();
    assert_eq!(header.matches("<w:tc>").count(), 3);
    assert!(header.contains(">Flag</w:t>"));
}
