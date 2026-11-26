//! 原本の罫線小数座標からWord表寸法を求め、整数ptの推定キーと分離する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 99.5ptと101ptの列、29.5ptと30.5ptの行を持つ正式な二行二列表を生成する。
fn source() -> Vec<u8> {
    controlled("", "")
}

/// 任意の正立変換と追加罫線を独立に制御する。
fn controlled(prefix: &str, suffix: &str) -> Vec<u8> {
    font_controlled(prefix, suffix, "Helvetica", 10.)
}

/// 代替fontとWordのhalf-point量子化を、罫線条件とは独立に制御する。
fn font_controlled(prefix: &str, suffix: &str, font: &str, size: f32) -> Vec<u8> {
    let content = "1 w 72.25 590.25 m 272.75 590.25 l S 72.25 620.75 m 272.75 620.75 l S 72.25 650.25 m 272.75 650.25 l S 72.25 590.25 m 72.25 650.25 l S 171.75 590.25 m 171.75 650.25 l S 272.75 590.25 m 272.75 650.25 l S BT /F 10 Tf 82 633 Td (Alpha) Tj ET BT /F 10 Tf 82 603 Td (Beta) Tj ET BT /F 10 Tf 182 603 Td (Control) Tj ET";
    let content = content.replace("/F 10 Tf", &format!("/F {size} Tf"));
    let content = format!("q {prefix} {content} {suffix} Q");
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} >>"),
    ])
}

/// 未埋め込みの日本語代替fontでは、狭くなる原本寸法へ無条件に切り替えない。
#[test]
fn unverified_fallback_font_keeps_compatible_table_dimensions() {
    let s = xml(&font_controlled("", "", "MS-PGothic", 10.));
    assert!(s.contains("<w:gridCol w:w=\"2000\""), "{s}");
    assert!(!s.contains("<w:gridCol w:w=\"1990\""));
    assert!(s.contains("<w:trHeight w:val=\"580\""));
    for text in ["Alpha", "Beta", "Control"] {
        assert_eq!(s.matches(&format!(">{text}</w:t>")).count(), 1);
    }
}

/// 原本の文字高がWordのhalf-pointへ変わる代替fontにも精密化を適用しない。
#[test]
fn quantized_fallback_size_keeps_compatible_table_dimensions() {
    let s = xml(&font_controlled("", "", "Helvetica", 10.25));
    assert!(s.contains("<w:gridCol w:w=\"2000\""), "{s}");
    assert!(!s.contains("<w:gridCol w:w=\"1990\""));
}

/// 列幅は推定キーの100/101ptでなく、原本の99.5/101ptからtwipへ一度だけ丸める。
#[test]
fn editable_table_keeps_fractional_source_column_widths() {
    let xml = xml(&source());
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    assert!(
        xml.contains("Alpha</w:t>") && xml.contains("Beta</w:t>") && xml.contains("Control</w:t>")
    );
    assert!(
        xml.contains("<w:gridCol w:w=\"1990\""),
        "the first column must use its 99.5pt source interval: {xml}"
    );
    assert!(xml.contains("<w:gridCol w:w=\"2020\""));
}

/// 製品byte APIのWord本文を、書換えずに読む。
fn xml(source: &[u8]) -> String {
    let bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-fractional-table.pdf", source).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 行の高さも、原本29.5/30.5ptの順序を保つ。
#[test]
fn source_row_heights_are_not_rounded_to_whole_points() {
    let s = xml(&source());
    assert!(
        s.contains("<w:trHeight w:val=\"590\" w:hRule=\"exact\""),
        "{s}"
    );
    assert!(s.contains("<w:trHeight w:val=\"610\" w:hRule=\"exact\""));
}

/// 既存の整数境界一つに異なる原本線が対応する場合は、勝手にどちらかを選ばない。
#[test]
fn ambiguous_parallel_border_keeps_integer_layout_fallback() {
    let s = xml(&controlled("", "72.4 590.25 m 72.4 650.25 l S"));
    assert!(s.contains("<w:gridCol w:w=\"2000\""), "{s}");
    assert!(!s.contains("<w:gridCol w:w=\"1990\""));
    assert!(s.contains("Alpha</w:t>") && s.contains("Beta</w:t>") && s.contains("Control</w:t>"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 平行移動で整数キーが変わっても、元の二列の幅と上から下の行高は変わらない。
    #[test]
    fn fractional_translation_preserves_source_table_dimensions(dx in -40i16..80,dy in -40i16..80) {
        let dx=f32::from(dx)/4.;let dy=f32::from(dy)/4.;
        let s=xml(&controlled(&format!("1 0 0 1 {dx} {dy} cm"),""));
        prop_assert_eq!(s.matches("<w:tbl>").count(),1);
        prop_assert!(s.contains("<w:gridCol w:w=\"1990\""));
        prop_assert!(s.contains("<w:gridCol w:w=\"2020\""));
        prop_assert!(s.contains("<w:trHeight w:val=\"590\""));
        prop_assert!(s.contains("<w:trHeight w:val=\"610\""));
        for text in ["Alpha","Beta","Control"] {prop_assert_eq!(s.matches(&format!(">{text}</w:t>")).count(),1);}
    }
}
