//! Font の glyph 幅を Unicode 文字数やページ回転に依存せず使う回帰。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// Font と任意の補助 object を持つ正式なページ/Form を生成する。
fn parse(font: &str, content: &str, in_form: bool, extra: &[String]) -> ParsedDocument {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(if in_form { "/Fm Do" } else { content });
    objects[5] = stream("");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push(font.into());
    objects.extend_from_slice(extra);
    parse_pdf_bytes("font-widths.pdf", &pdf(&objects)).unwrap()
}

/// 水平文字にも Helvetica AFM の W/i 幅を適用する。
#[test]
fn helvetica_width_is_not_rotation_dependent() {
    for in_form in [false, true] {
        let parsed = parse(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
            "BT /F 10 Tf 10 10 Td (Wi) Tj ET",
            in_form,
            &[],
        );
        assert!((parsed.text_elements[0].bounds.x1 - 21.66).abs() < 0.0001);
    }
}

/// simple Font の間接 Subtype も標準幅の判定に使う。
#[test]
fn indirect_simple_subtype_uses_standard_widths() {
    let parsed = parse(
        "<< /Type /Font /Subtype 11 0 R /BaseFont /Courier >>",
        "BT /F 10 Tf 10 10 Td (AB) Tj ET",
        true,
        &["/Type1".into()],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 22.);
}

/// Courier の全 glyph は600 units/emであり、後続描画も同じ幅で進む。
#[test]
fn courier_advance_uses_six_tenths_em() {
    let parsed = parse(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
        "BT /F 10 Tf 10 10 Td (AB) Tj /F 20 Tf (C) Tj ET",
        true,
        &[],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 22.);
    assert_eq!(parsed.text_elements[1].bounds.x0, 22.);
    assert_eq!(parsed.text_elements[1].bounds.x1, 34.);
}

/// Widths は標準 Font の幅より優先し、FirstChar で encoded code に対応する。
#[test]
fn explicit_widths_override_standard_metrics() {
    for in_form in [false, true] {
        let parsed = parse(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /FirstChar 65 /LastChar 66 /Widths [900 300] >>",
            "BT /F 10 Tf 10 10 Td (AB) Tj /F 20 Tf (A) Tj ET",
            in_form,
            &[],
        );
        assert_eq!(parsed.text_elements[0].bounds.x1, 22.);
        assert_eq!(parsed.text_elements[1].bounds.x0, 22.);
        assert_eq!(parsed.text_elements[1].bounds.x1, 40.);
    }
}

/// 間接 Widths/数値と FontDescriptor の MissingWidth を解決する。
#[test]
fn indirect_widths_and_missing_width_are_used() {
    let parsed = parse(
        "<< /Type /Font /Subtype /TrueType /BaseFont /Custom /FirstChar 65 /LastChar 65 /Widths 11 0 R /FontDescriptor 13 0 R >>",
        "BT /F 10 Tf 10 10 Td (AZ) Tj ET",
        true,
        &[
            "[12 0 R]".into(),
            "800".into(),
            "<< /Type /FontDescriptor /MissingWidth 250 >>".into(),
        ],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 20.5);
}

/// ToUnicode の一対多変換でも、glyph 幅は復号前の一 byte から求める。
#[test]
fn unicode_ligature_does_not_multiply_glyph_width() {
    let cmap = stream("1 beginbfchar\n<41> <00660069>\nendbfchar");
    for show in ["(A)", "<41>"] {
        let parsed = parse(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Custom /FirstChar 65 /LastChar 65 /Widths [700] /ToUnicode 11 0 R >>",
            &format!("BT /F 10 Tf 10 10 Td {show} Tj ET"),
            true,
            std::slice::from_ref(&cmap),
        );
        assert_eq!(parsed.text_elements[0].text, "fi");
        assert_eq!(parsed.text_elements[0].bounds.x1, 17.);
    }
}

/// オクタル escape の上位 byte は UTF-8 に変換せず、一つの glyph code にする。
#[test]
fn octal_high_byte_keeps_one_encoded_glyph() {
    let parsed = parse(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Custom /FirstChar 128 /LastChar 128 /Widths [800] /ToUnicode 11 0 R >>",
        "BT /F 10 Tf 10 10 Td (\\200) Tj ET",
        true,
        &[stream("1 beginbfchar\n<80> <0041>\nendbfchar")],
    );
    assert_eq!(parsed.text_elements[0].text, "A");
    assert_eq!(parsed.text_elements[0].bounds.x1, 18.);
}

/// StandardEncoding の左右引用符と WinAnsi の quotesingle/grave を区別する。
#[test]
fn helvetica_quote_width_follows_encoding() {
    for (encoding, end) in [
        ("", 14.44),
        ("/Encoding /StandardEncoding", 14.44),
        ("/Encoding /WinAnsiEncoding", 15.24),
        ("/Encoding << /BaseEncoding /MacRomanEncoding >>", 15.24),
    ] {
        let parsed = parse(
            &format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica {encoding} >>"),
            "BT /F 10 Tf 10 10 Td <2760> Tj ET",
            true,
            &[],
        );
        assert!((parsed.text_elements[0].bounds.x1 - end).abs() < 0.0001);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
    /// Tj/TJ 分割と無調整連結で同じ文字列・最終原点になる。
    #[test]
    fn splitting_preserves_width_and_advance(count in 1usize..20) {
        let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>";
        let whole = parse(font, &format!("BT /F 10 Tf 10 10 Td ({}) Tj /F 20 Tf (Z) Tj ET", "A".repeat(count)), true, &[]);
        for show in ["(A) Tj ".repeat(count), format!("[{}] TJ", "(A) ".repeat(count))] {
            let split = parse(font, &format!("BT /F 10 Tf 10 10 Td {show} /F 20 Tf (Z) Tj ET"), true, &[]);
            prop_assert_eq!(&whole.text_elements, &split.text_elements);
            prop_assert_eq!(split.text_elements[1].bounds.x0, 10. + count as f32 * 6.);
        }
    }
}
