//! 横並びの独立した原本表を、編集可能な二つのWord表のまま配置する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 二行二列の表二つと、表後の本文を持つ所有原本。
fn source() -> Vec<u8> {
    source_at(0, 360, 0, false)
}

/// 水平移動・共通上端・段差・表間本文を変えた所有原本を構成する。
fn source_at(dx: i32, top: i32, stagger: i32, prose: bool) -> Vec<u8> {
    let mut content = format!("BT /F1 8 Tf {} {} Td (Heading) Tj ET ", 30 + dx, top + 20);
    for (x, prefix) in [(30, "A"), (160, "B")] {
        let x = x + dx;
        let top = if prefix == "B" { top - stagger } else { top };
        for y in [top - 60, top - 30, top] {
            content.push_str(&format!("{x} {y} m {} {y} l S ", x + 100));
        }
        for left in [x, x + 50, x + 100] {
            content.push_str(&format!("{left} {} m {left} {top} l S ", top - 60));
        }
        for (index, (dx, y)) in [(5, 345), (55, 345), (5, 315), (55, 315)]
            .into_iter()
            .enumerate()
        {
            content.push_str(&format!(
                "BT /F1 8 Tf {} {} Td ({prefix}{index}) Tj ET ",
                x + dx,
                y + top - 360
            ));
        }
    }
    content.push_str(&format!(
        "1 g {} {} 30 1 re f 0 g BT /F1 8 Tf {} {} Td (After) Tj ET",
        130 + dx,
        top as f32 - 30.5,
        30 + dx,
        top - 80
    ));
    if prose {
        content.push_str(&format!(
            " BT /F1 8 Tf {} {} Td (W) Tj ET",
            140 + dx,
            top - 25
        ));
    }
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 300 420] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        page_attribute_pdf::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ])
}

/// 公開変換から、編集可能なWordの本文XMLを読む。
fn xml(pdf: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-adjacent.pdf", pdf).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 結合・行列・本文を持つ二つの表を、同じ原本の上端へ配置する。
#[test]
fn distinct_source_tables_keep_shared_top_and_editable_cells() {
    let xml = xml(&source());
    assert_eq!(xml.matches("<w:tbl>").count(), 2, "{xml}");
    assert_eq!(xml.matches("<w:tr>").count(), 4);
    for text in [
        "A0", "A1", "A2", "A3", "B0", "B1", "B2", "B3", "Heading", "After",
    ] {
        assert_eq!(xml.matches(&format!(">{text}</w:t>")).count(), 1, "{text}");
    }
    assert_eq!(
        xml.matches("<w:tblpPr").count(),
        2,
        "independent tables must share their source top: {xml}"
    );
    assert_eq!(xml.matches("w:tblpY=\"1200\"").count(), 2);
    for x in [600, 3200] {
        assert!(xml.contains(&format!("w:tblpX=\"{x}\"")));
    }
}

/// 上端が異なる表や、表間の帯に本文を持つ原本へ浮動配置を推測しない。
#[test]
fn staggered_tables_and_intervening_prose_keep_existing_flow() {
    for pdf in [source_at(0, 360, 10, false), source_at(0, 360, 0, true)] {
        let xml = xml(&pdf);
        assert_eq!(xml.matches("<w:tbl>").count(), 2);
        assert!(!xml.contains("<w:tblpPr"), "{xml}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    /// 原本を平行移動しても、セルや行を増やさず共通上端と別の左端を追従する。
    #[test]
    fn translated_tables_preserve_source_coordinates(dx in 0i32..16, top in 310i32..371) {
        let xml=xml(&source_at(dx,top,0,false));
        prop_assert_eq!(xml.matches("<w:tbl>").count(),2);
        prop_assert_eq!(xml.matches("<w:tr>").count(),4);
        prop_assert_eq!(xml.matches(&format!("w:tblpY=\"{}\"",(420-top)*20)).count(),2);
        for x in [30+dx,160+dx] { prop_assert!(xml.contains(&format!("w:tblpX=\"{}\"",x*20)), "source x={}", x); }
    }
}
