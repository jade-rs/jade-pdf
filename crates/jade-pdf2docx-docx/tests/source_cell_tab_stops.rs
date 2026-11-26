//! 同じセルの離れた文字群を、空白数の推測でなく原本位置へ保つ。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 二行二列表の一セルに、60pt離れた同じbaselineの文字群を描く。
fn source() -> Vec<u8> {
    controlled("", "Helvetica", 142.)
}

/// font・平行移動・後続原点を独立に変更して位置の推測を検証する。
fn controlled(prefix: &str, font: &str, next: f32) -> Vec<u8> {
    let content = "1 w 72 590 m 372 590 l S 72 620 m 372 620 l S 72 650 m 372 650 l S 72 590 m 72 650 l S 272 590 m 272 650 l S 372 590 m 372 650 l S BT /F 10 Tf 82 633 Td (New) Tj ET BT /F 10 Tf 142 633 Td (Old) Tj ET BT /F 10 Tf 82 603 Td (Control) Tj ET BT /F 10 Tf 282 603 Td (End) Tj ET";
    let content = format!(
        "q {prefix} {} Q",
        content.replace("142 633", &format!("{next} 633"))
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} >>"),
    ])
}

/// 製品APIから得たWord XMLを、字形・空白・位置の書換えなしに読む。
fn xml(source: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-tabs.pdf", source).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    out
}

/// セル左端から70ptの後続文字原点を明示し、二つの数値群を詰めない。
#[test]
fn separated_cell_groups_keep_their_source_tab_position() {
    let s = xml(&source());
    assert_eq!(s.matches("<w:tbl>").count(), 1);
    assert!(s.contains("w:val=\"left\" w:pos=\"1400\""), "{s}");
    assert!(s.contains("<w:tab />"), "{s}");
    for text in ["New", "Old", "Control", "End"] {
        assert_eq!(s.matches(&format!(">{text}</w:t>")).count(), 1);
    }
}

/// 未確認の代替fontと連続本文には、原本の字幅が確認済みというタブを付けない。
#[test]
fn unknown_fonts_and_small_word_gaps_keep_compatible_paragraphs() {
    for pdf in [
        controlled("", "MS-PGothic", 142.),
        controlled("", "Helvetica", 105.),
    ] {
        let s = xml(&pdf);
        assert!(!s.contains("<w:tabs>"), "{s}");
        assert!(s.contains("New</w:t>") && s.contains("Old</w:t>"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 原点と空隙を変えても、セル相対位置と本文の一回性を保つ。
    #[test]
    fn translated_cell_preserves_relative_tab_stops(dx in -40i16..60, dy in -40i16..60, next in 140u16..200) {
        let s = xml(&controlled(&format!("1 0 0 1 {dx} {dy} cm"), "Helvetica", f32::from(next)));
        prop_assert_eq!(s.matches("<w:tbl>").count(), 1);
        prop_assert!(s.contains(&format!("w:val=\"left\" w:pos=\"{}\"", (next - 72) * 20)));
        prop_assert_eq!(s.matches("<w:tab />").count(), 1);
        for word in ["New", "Old", "Control", "End"] { prop_assert_eq!(s.matches(&format!(">{word}</w:t>")).count(), 1); }
    }
}
