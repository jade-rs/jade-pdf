//! 数値列の大きいTJ間隔を、一つの横長本文矩形へ潰さず原本位置を保つ。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 所有するCourier原本に、明示的な30pointの列間隔を持つ二つの値を置く。
fn source_with(content: &str) -> Vec<u8> {
    source_with_font(content, "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>")
}

/// 未確認幅の対照も、同じ所有object配置とxrefで作る。
fn source_with_font(content: &str, font: &str) -> Vec<u8> {
    let mut objects = pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> >>");
    objects[4] = stream(content);
    objects[5] = stream("");
    objects.push(font.into());
    pdf(&objects)
}

/// 幅表のない非標準fontをUnicode文字数から推定した配置は、新しい数値列分離へ使わない。
#[test]
fn unmeasured_font_widths_keep_numeric_operands_merged() {
    for font in [
        "<< /Type /Font /Subtype /Type1 /BaseFont /OwnedUnknown >>",
        "<< /Type /Font /Subtype /TrueType /BaseFont /OwnedUnknown >>",
    ] {
        let input = source_with_font("BT /F 10 Tf 20 100 Td [(376) -3000 (-2.4)] TJ ET", font);
        let parsed = parse_pdf_bytes("owned-unmeasured-numeric.pdf", &input).unwrap();
        assert_eq!(parsed.text_elements.len(), 1, "{font}");
        assert_eq!(parsed.text_elements[0].text, "376 -2.4");
    }
}

/// 表セルへの割当前に、別々の原本数値と各原点・幅を公開byte APIで確認する。
#[test]
fn wide_numeric_tj_gap_preserves_independent_source_positions() {
    let parsed = parse_pdf_bytes(
        "owned-numeric-tj.pdf",
        &source_with("BT /F 10 Tf 20 100 Td [(376) -3000 (-2.4)] TJ ET"),
    )
    .unwrap();
    let elements = parsed
        .text_elements
        .iter()
        .filter(|t| !t.text.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        elements.len(),
        2,
        "two numeric columns must retain independent source frames"
    );
    assert_eq!(elements[0].text, "376");
    assert_eq!(elements[1].text, "-2.4");
    assert_eq!((elements[0].bounds.x0, elements[0].bounds.x1), (20., 38.));
    assert_eq!((elements[1].bounds.x0, elements[1].bounds.x1), (68., 92.));
    assert_eq!(elements[0].baseline_y, elements[1].baseline_y);
}

/// 一em未満の数値間隔や広い散文は、元の単一描画要素と語間を保つ。
#[test]
fn small_numeric_gaps_and_wide_prose_keep_existing_merge() {
    for (operand, expected) in [
        ("[(376) -600 (-2.4)]", "376 -2.4"),
        ("[(Alpha) -3000 (Beta)]", "Alpha Beta"),
        ("[(1.2.3) -3000 (4.5)]", "1.2.3 4.5"),
    ] {
        let input = source_with(&format!("BT /F 10 Tf 20 100 Td {operand} TJ ET"));
        let parsed = parse_pdf_bytes("owned-non-column-tj.pdf", &input).unwrap();
        assert_eq!(parsed.text_elements.len(), 1);
        assert_eq!(parsed.text_elements[0].text, expected);
    }
}

/// 回転して縦方向へ送る数値を、正立した横の列と推測しない。
#[test]
fn rotated_numeric_gaps_keep_existing_merge() {
    let input = source_with("BT /F 10 Tf 0 1 -1 0 20 100 Tm [(376) -3000 (-2.4)] TJ ET");
    let parsed = parse_pdf_bytes("owned-rotated-numeric.pdf", &input).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "376 -2.4");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/production_numeric_tj_positions.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// fontsize・水平倍率・明示gapを変えても、元Courier幅と次の原点を同じ式で保つ。
    #[test]
    fn independent_numeric_frames_keep_font_size_scale_and_tj_advance(
        size in 5u16..25,
        adjustment in 1500u16..6000,
        scale in 50u16..200,
    ) {
        let input = source_with(&format!("BT /F {size} Tf {scale} Tz 20 100 Td [(376) -{adjustment} (-2.4)] TJ ET"));
        let parsed = parse_pdf_bytes("owned-numeric-advance.pdf", &input).unwrap();
        let elements = &parsed.text_elements;
        prop_assert_eq!(elements.len(), 2);
        let factor = f32::from(scale) / 100.;
        let size = f32::from(size);
        let right = 20. + (1.8 + f32::from(adjustment) / 1000.) * size * factor;
        prop_assert_eq!(elements[0].text.as_str(), "376");
        prop_assert_eq!(elements[1].text.as_str(), "-2.4");
        prop_assert!((elements[0].bounds.x1 - (20. + 1.8 * size * factor)).abs() < 0.001);
        prop_assert!((elements[1].bounds.x0 - right).abs() < 0.001);
        prop_assert!((elements[1].bounds.x1 - (right + 2.4 * size * factor)).abs() < 0.001);
        prop_assert_eq!(elements[0].baseline_y, elements[1].baseline_y);
    }
}
