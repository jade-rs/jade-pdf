//! FontDescriptorの通常weightと、原本の同色fill-and-stroke強調を区別する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 同じweight400のFontで、細い同色strokeを重ねた文字と通常の文字を描く。
fn source() -> Vec<u8> {
    source_with(
        "0 g 0 G .24 w BT /F1 12 Tf 1 0 0 1 30 180 Tm 2 Tr (Painted) Tj 0 Tr 1 0 0 1 30 150 Tm (Regular) Tj ET",
        "",
    )
}

/// 所有する本文と所属Resourceだけから、正式な原本を構成する。
fn source_with(content: &str, resources: &str) -> Vec<u8> {
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 300 240] >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> {resources} >> /Contents 4 0 R >>"),
        page_attribute_pdf::stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>".into(),
        "<< /Type /FontDescriptor /FontName /Courier /FontWeight 400 /Flags 32 /FontBBox [0 -200 600 800] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>".into(),
    ])
}

/// 公開byte APIのWord本文から、対象runの太字指定を読み出す。
fn word_bold(source: &[u8], word: &str) -> bool {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-paint-scope.pdf", source).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let run = xml
        .split("<w:r>")
        .find(|part| part.contains(&format!(">{word}</w:t>")))
        .unwrap()
        .split("</w:r>")
        .next()
        .unwrap();
    run.contains("<w:b />") || run.contains("<w:b w:val=\"true\"")
}

/// 実際のFontDescriptorは通常体のまま、細い同色の原本強調だけをWordへ渡す。
#[test]
fn source_paint_emphasis_does_not_rewrite_declared_font_weight() {
    let source = source();
    let parsed = parse_pdf_bytes("owned-paint-emphasis.pdf", &source).unwrap();
    for word in ["Painted", "Regular"] {
        let element = parsed
            .text_elements
            .iter()
            .find(|t| t.text == word)
            .unwrap();
        assert_eq!(
            element.font_weight,
            Some(400),
            "{word}: retain actual descriptor weight"
        );
        assert_eq!(
            element.fill_stroke_width,
            if word == "Painted" { Some(0.24) } else { None }
        );
    }
    let bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-paint-emphasis.pdf", &source).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    for (word, bold) in [("Painted", true), ("Regular", false)] {
        let run = xml
            .split("<w:r>")
            .find(|part| part.contains(&format!(">{word}</w:t>")))
            .unwrap()
            .split("</w:r>")
            .next()
            .unwrap();
        let actual = run.contains("<w:b />") || run.contains("<w:b w:val=\"true\"");
        assert_eq!(actual, bold, "{word}: {run}");
    }
}

/// 同色・不透明・細い塗りとstrokeだけをWord太字へ対応させる。
#[test]
fn unknown_color_effects_and_other_modes_do_not_infer_bold() {
    for prefix in [
        "0 g 0 G .24 w 0 Tr",
        "0 g 0 G .24 w 1 Tr",
        "0 g 1 G .24 w 2 Tr",
        "0 g 0 G 0 w 2 Tr",
        "0 g 0 G 1 w 2 Tr",
        "0 g 0 G .24 w /DeviceGray CS 0 SC 2 Tr",
        "/Unknown gs 0 g 0 G .24 w 2 Tr",
        "2 0 0 1 0 0 cm 0 g 0 G .24 w 2 Tr",
    ] {
        let input = source_with(
            &format!("{prefix} BT /F1 12 Tf 30 180 Td (Painted) Tj ET"),
            "",
        );
        assert!(!word_bold(&input, "Painted"), "{prefix}");
    }
    for (prefix, resources) in [
        ("/P gs", "/ExtGState << /P << /ca .5 >> >>"),
        ("/P gs", "/ExtGState << /P << /CA .5 >> >>"),
        ("/P gs", "/ExtGState << /P << /BM /Multiply >> >>"),
        ("/P gs", "/ExtGState << /P << /TR /Identity >> >>"),
        ("", "/ColorSpace << /DefaultGray /DeviceGray >>"),
    ] {
        let input = source_with(
            &format!("{prefix} 0 g 0 G .24 w BT /F1 12 Tf 30 180 Td 2 Tr (Painted) Tj ET"),
            resources,
        );
        let parsed = parse_pdf_bytes("owned-effects.pdf", &input).unwrap();
        assert!(
            parsed
                .text_elements
                .iter()
                .all(|t| t.fill_stroke_width.is_none())
        );
        assert!(!word_bold(&input, "Painted"), "{resources}");
    }
}

/// Tfを1としTmでemを拡大する原本でも、実効font高に対する線幅を使う。
#[test]
fn text_matrix_em_size_and_normal_print_state_are_supported() {
    let input = source_with(
        "/P gs 0 g 0 G .24 w BT /F1 1 Tf 12 0 0 12 30 180 Tm 2 Tr (Painted) Tj ET",
        "/ExtGState << /P << /CA 1 /ca 1 /BG2 /Default /UCR2 /Default /OP false /op false /OPM 1 /SA false /SM .02 >> >>",
    );
    let parsed = parse_pdf_bytes("owned-Tm.pdf", &input).unwrap();
    let t = parsed
        .text_elements
        .iter()
        .find(|t| t.text == "Painted")
        .unwrap();
    assert_eq!(t.font_size, 12.);
    assert_eq!(t.fill_stroke_width, Some(0.24));
    assert!(word_bold(&input, "Painted"));
}

/// q/Qで別色のstrokeを戻し、後続の確認済み同色の強調だけを残す。
#[test]
fn saved_graphics_paint_restores_stroke_color_and_width() {
    let input = source_with(
        "0 g 0 G .24 w q 1 G .36 w BT /F1 12 Tf 30 180 Td 2 Tr (Different) Tj ET Q BT /F1 12 Tf 30 150 Td 2 Tr (Painted) Tj ET",
        "",
    );
    assert!(!word_bold(&input, "Different"));
    assert!(word_bold(&input, "Painted"));
}

/// 同じbaselineで通常体へ戻した文字を、前の強調runへ結合しない。
#[test]
fn adjacent_normal_text_does_not_inherit_paint_emphasis() {
    let input = source_with(
        "0 g 0 G .24 w BT /F1 12 Tf 30 180 Td 2 Tr (Painted) Tj 0 Tr (Regular) Tj ET",
        "",
    );
    let parsed = parse_pdf_bytes("owned-adjacent-emphasis.pdf", &input).unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>(),
        ["Painted", "Regular"]
    );
    assert!(word_bold(&input, "Painted"));
    assert!(!word_bold(&input, "Regular"));
}

/// nativeでもdescriptor400を書き換えず、照合できたsourceの描画幅を保持する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_reconciliation_keeps_source_paint_and_regular_weight() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned-native-paint.pdf");
    std::fs::write(&path, source()).unwrap();
    let parsed = jade_pdf2docx_parser::parse_pdf(&path).unwrap();
    for (word, width) in [("Painted", Some(0.24)), ("Regular", None)] {
        let element = parsed
            .text_elements
            .iter()
            .find(|t| t.text == word)
            .unwrap();
        assert_eq!(element.font_weight, Some(400));
        assert_eq!(element.fill_stroke_width, width);
    }
}

/// 編集可能な罫線表へ割り当てた後も、通常体と描画による強調をセルごとに保持する。
#[test]
fn editable_table_cells_keep_painted_and_regular_styles() {
    let mut content = String::from("0 g 0 G .5 w ");
    for y in [100, 130, 160] {
        content.push_str(&format!("30 {y} m 130 {y} l S "));
    }
    for x in [30, 80, 130] {
        content.push_str(&format!("{x} 100 m {x} 160 l S "));
    }
    content.push_str(".2 w BT /F1 10 Tf 35 140 Td 2 Tr (Painted) Tj ET BT /F1 10 Tf 85 140 Td 0 Tr (Regular) Tj ET BT /F1 10 Tf 35 110 Td (1) Tj ET BT /F1 10 Tf 85 110 Td (2) Tj ET");
    let input = source_with(&content, "");
    assert!(word_bold(&input, "Painted"));
    assert!(!word_bold(&input, "Regular"));
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-painted-cells.pdf", &input).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    assert_eq!(xml.matches("<w:tr>").count(), 2);
    assert_eq!(xml.matches("<w:tc>").count(), 4);
}

/// 字種もfont種別も確認できない既存の回復文字列から、重ね描きの強調を推測しない。
#[test]
fn missing_selected_font_cannot_confirm_paint_emphasis() {
    let input = source_with("0 g 0 G .24 w BT 30 180 Td 2 Tr (Painted) Tj ET", "");
    let parsed = parse_pdf_bytes("owned-unknown-font.pdf", &input).unwrap();
    assert!(
        parsed
            .text_elements
            .iter()
            .all(|t| t.fill_stroke_width.is_none())
    );
    assert!(!word_bold(&input, "Painted"));
}

/// 同じDevice色とPDFの成分clipを確認できる黒だけを、通常体から独立して強調する。
#[test]
fn same_device_rgb_cmyk_and_clamped_gray_confirm_paint() {
    for color in ["0 0 0 rg 0 0 0 RG", "0 0 0 1 k 0 0 0 1 K", "-1 g 0 G"] {
        let input = source_with(
            &format!("{color} .24 w BT /F1 12 Tf 30 180 Td 2 Tr (Painted) Tj ET"),
            "",
        );
        let parsed = parse_pdf_bytes("owned-device-paint.pdf", &input).unwrap();
        assert_eq!(parsed.text_elements[0].fill_stroke_width, Some(0.24));
        assert!(word_bold(&input, "Painted"), "{color}");
    }
}

/// Type3のglyph内部のpaintはTrと独立しており、外側の同色strokeから太字を作らない。
#[test]
fn type3_character_program_does_not_infer_outer_paint_emphasis() {
    let input=page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 300 240] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        page_attribute_pdf::stream("0 g 0 G .24 w BT /F1 12 Tf 30 180 Td 2 Tr (A) Tj ET"),
        "<< /Type /Font /Subtype /Type3 /Name /Owned /FontBBox [0 0 600 700] /FontMatrix [.001 0 0 .001 0 0] /CharProcs << /A 6 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [600] /Resources << >> >>".into(),
        page_attribute_pdf::stream("600 0 0 0 600 700 d1 0 0 600 700 re f"),
    ]);
    let parsed = parse_pdf_bytes("owned-Type3-paint.pdf", &input).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "A");
    assert!(parsed.text_elements[0].fill_stroke_width.is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 一様な拡大でも原本weightを変えず、em比2%の線幅による強調を保つ。
    #[test]
    fn uniform_scale_preserves_explicit_paint_emphasis(size in 6u16..17, scale in 1u16..5) {
        let width=f32::from(size)*0.02;
        let input=source_with(&format!("{scale} 0 0 {scale} 0 0 cm 0 g 0 G {width:.5} w BT /F1 {size} Tf 5 5 Td 2 Tr (Painted) Tj ET"),"");
        let parsed=parse_pdf_bytes("owned-scaled-paint.pdf",&input).unwrap();let t=parsed.text_elements.iter().find(|t|t.text=="Painted").unwrap();
        prop_assert_eq!(t.font_weight,Some(400));prop_assert!((t.fill_stroke_width.unwrap()-width*f32::from(scale)).abs()<0.0001);prop_assert!(word_bold(&input,"Painted"));
    }
}
