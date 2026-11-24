//! 原本で右端が揃う複数の数値行を、狭い左indentの残り幅へ押し込まず右端に対応づける。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};

/// Helveticaの公開AFM幅で、別々の桁数を同じ右端へ描く所有表を作る。
fn source(left: u16, width: u16, size: u16, inset: u16, right: bool) -> Vec<u8> {
    let middle = left + width;
    let end = middle + width;
    let mut content = format!(
        "1 w {left} 560 m {end} 560 l S {left} 650 m {end} 650 l S {left} 560 m {left} 650 l S {middle} 560 m {middle} 650 l S {end} 560 m {end} 650 l S "
    );
    for (i, (text, em)) in [("9", 0.556), ("82", 1.112), ("1,234", 2.502)]
        .into_iter()
        .enumerate()
    {
        let x = if right {
            f64::from(middle - inset) - em * f64::from(size)
        } else {
            f64::from(left + 5)
        };
        let y = 630 - i * 25;
        content.push_str(&format!("BT /F1 {size} Tf {x:.5} {y} Td ({text}) Tj ET "));
    }
    content.push_str(&format!(
        "BT /F1 {size} Tf {} 630 Td (Control) Tj ET ",
        middle + 5
    ));
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()),
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
    bytes
}

/// 製品byte APIが出力した最初のセルだけを読む。
fn cell(left: u16, width: u16, size: u16, inset: u16, right: bool) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-right-aligned-numeric-lines.pdf",
        &source(left, width, size, inset, right),
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let begin = xml.find("<w:tc>").unwrap();
    let end = begin + xml[begin..].find("</w:tc>").unwrap();
    xml[begin..end].to_string()
}

/// 原本の2ptの右余白と、可変桁の三つの数値を同時に保持する。
#[test]
fn source_right_aligned_numbers_keep_their_right_anchor() {
    let xml = cell(72, 80, 10, 2, true);
    assert_eq!(
        xml.matches("<w:jc w:val=\"right\"").count(),
        3,
        "source numeric column must retain verified right anchors"
    );
    assert_eq!(xml.matches("<w:ind w:left=\"0\" w:right=\"40\"").count(), 3);
    for text in ["9", "82", "1,234"] {
        assert_eq!(xml.matches(&format!(">{text}</w:t>")).count(), 1);
    }
    assert_eq!(xml.matches("w:sz w:val=\"20\"").count(), 3);
}

/// 数値であるだけでは、左端が揃う原本を右寄せに変えない。
#[test]
fn source_left_aligned_numbers_keep_their_left_anchor() {
    let xml = cell(72, 80, 10, 2, false);
    assert!(!xml.contains("<w:jc w:val=\"right\""));
    assert_eq!(xml.matches("<w:ind w:left=\"100\"").count(), 3);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 表の位置・列幅・書体サイズ・右余白を変えても、右端のsource座標と本文は不変である。
    #[test]
    fn numeric_right_anchor_is_independent_of_translation_and_cell_width(left in 20u16..150,width in 45u16..100,size in 8u16..15,inset in 1u16..5) {
        let xml=cell(left,width,size,inset,true);
        prop_assert_eq!(xml.matches("<w:jc w:val=\"right\"").count(),3);
        let expected=format!("<w:ind w:left=\"0\" w:right=\"{}\"",inset*20);
        prop_assert_eq!(xml.matches(&expected).count(),3);
        for text in ["9","82","1,234"] {prop_assert_eq!(xml.matches(&format!(">{text}</w:t>")).count(),1);}
    }
}
