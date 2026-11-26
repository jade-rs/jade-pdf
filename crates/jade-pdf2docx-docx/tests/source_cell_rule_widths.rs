//! 原本の太い外枠と細い内罫線を、同じWord表でも各辺へ個別に保持する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};

#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 1.5ptの外枠と0.5ptの内罫線を持ち、各セルの所有が明確な原本を作る。
fn source(prefix: &str, font: &str) -> Vec<u8> {
    let content = format!(
        "q {prefix} 1.5 w 72.25 590.25 200.5 60 re S .5 w 171.75 590.25 m 171.75 650.25 l S 72.25 620.75 m 272.75 620.75 l S BT /F 10 Tf 82 633 Td (Alpha) Tj ET BT /F 10 Tf 82 603 Td (Beta) Tj ET BT /F 10 Tf 182 603 Td (Control) Tj ET Q"
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} >>"),
    ])
}

/// byte APIの出力から、書換えずにセルごとのWord本文を読む。
fn cells(prefix: &str, font: &str) -> Vec<String> {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-source-rule-widths.pdf",
        &source(prefix, font),
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml.split("<w:tc>")
        .skip(1)
        .map(|s| s.split("</w:tc>").next().unwrap().to_owned())
        .collect()
}

/// 全辺を最大線幅へ置換せず、物理的な各辺の原本線幅を保持する。
#[test]
fn outer_and_inner_source_rules_keep_independent_widths() {
    let cells = cells("", "Helvetica");
    assert_eq!(cells.len(), 4);
    let expected = [
        [12, 12, 4, 4],
        [12, 4, 4, 12],
        [4, 12, 12, 4],
        [4, 4, 12, 12],
    ];
    for (cell, sizes) in cells.iter().zip(expected) {
        for (edge, size) in ["top", "left", "bottom", "right"].into_iter().zip(sizes) {
            assert!(
                cell.contains(&format!("<w:{edge} w:val=\"single\" w:sz=\"{size}\"")),
                "source {edge} must retain {size}/8pt: {cell}"
            );
        }
    }
    assert!(cells[0].contains("Alpha</w:t>"));
    assert!(cells[2].contains("Beta</w:t>"));
    assert!(cells[3].contains("Control</w:t>"));
}

/// 未確認の代替fontで精密gridへ進めない表は、新しい線幅対応の根拠にも使わない。
#[test]
fn unverified_fallback_keeps_legacy_border_layout() {
    let cells = cells("", "MS-PGothic");
    assert_eq!(cells.len(), 4);
    for cell in cells {
        for edge in ["top", "left", "bottom", "right"] {
            assert!(cell.contains(&format!("<w:{edge} w:val=\"single\" w:sz=\"12\"")));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 罫線と文字の平行移動は、線幅の種類や元のセル所有を変えない。
    #[test]
    fn translation_preserves_source_side_widths(dx in -40i16..80, dy in -40i16..80) {
        let cells=cells(&format!("1 0 0 1 {} {} cm",f32::from(dx)/4.,f32::from(dy)/4.),"Helvetica");
        prop_assert_eq!(cells.len(),4);
        let all=cells.concat();
        prop_assert_eq!(all.matches("w:val=\"single\" w:sz=\"4\"").count(),8);
        prop_assert_eq!(all.matches("w:val=\"single\" w:sz=\"12\"").count(),8);
        for text in ["Alpha","Beta","Control"] {prop_assert_eq!(all.matches(&format!(">{text}</w:t>")).count(),1);}
    }
}
