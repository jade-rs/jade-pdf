//! native矩形抽出の重複回復・反復保持・制御glyph保護を確認する。
use super::budget_tests::{budget, owned_text_pdf, with_native_bytes, with_page};
use super::{DecodeBudget, characters, index_preserves_controls, matches_preview};
use crate::{
    ParseLimits, ParsedTextElement,
    page_attribute_fixture::{pdf, stream},
};

/// baselineが少し異なる隣接glyphを矩形抽出が再読しても、公開native結果で重複しない。
#[test]
fn native_logo_extracts_each_sdk_character_once() {
    for (lowered_x, final_x) in [(38., 44.), (39., 45.), (40., 46.)] {
        let content = format!(
            "BT /F 12 Tf 1 0 0 1 20 100 Tm (LaT) Tj 1 0 0 1 {lowered_x} 97.65 Tm (E) Tj 1 0 0 1 {final_x} 100 Tm (X) Tj ET"
        );
        with_page(&content, |text| {
            assert_eq!(
                text.all()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>(),
                "LaTEX"
            );
        });
        let limits = ParseLimits::default();
        let parsed = crate::pdfium_backend::parse_pdf(
            std::path::Path::new("owned-lowered-glyph.pdf"),
            &owned_text_pdf(&content),
            &limits,
            &DecodeBudget::new(&limits),
        )
        .unwrap();
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .flat_map(|e| e.text.chars())
                .filter(|c| !c.is_whitespace())
                .collect::<String>(),
            "LaTEX",
            "native output must contain each SDK character once"
        );
    }
}

/// 矩形の重複を回復したページでも、同じsource text objectの見出しを文字単位へ崩さない。
#[test]
fn native_recovery_retains_a_coherent_heading_span() {
    let title = "Coherent native heading";
    let content = format!(
        "BT /F 12 Tf 1 0 0 1 20 150 Tm ({title}) Tj \
         1 0 0 1 20 100 Tm (LaT) Tj 1 0 0 1 38 97.65 Tm (E) Tj \
         1 0 0 1 44 100 Tm (X) Tj 1 0 0 1 160 40 Tm (z) Tj \
         1 0 0 1 160 40 Tm (z) Tj ET"
    );
    // 同じ場所のzをSDKが一つにするため、pureとの文字数照合ではfallbackが必要になる。
    with_page(&content, |text| {
        assert_eq!(
            text.all()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>(),
            format!("{}LaTEXz", title.replace(' ', ""))
        );
    });
    let limits = ParseLimits::default();
    let parsed = crate::pdfium_backend::parse_pdf(
        std::path::Path::new("owned-coherent-native-heading.pdf"),
        &owned_text_pdf(&content),
        &limits,
        &DecodeBudget::new(&limits),
    )
    .unwrap();
    let heading = parsed
        .text_elements
        .iter()
        .filter(|e| e.bounds.y0 > 140.)
        .collect::<Vec<_>>();
    assert_eq!(
        heading.len(),
        1,
        "indexed recovery must retain the coherent source heading span"
    );
    assert_eq!(heading[0].text.trim(), title);
}

/// 照合は確保不要でも停止済み予算を尊重し、反復・並び替え・未知文字を同一視しない。
#[test]
fn preview_comparison_preserves_repetition_unicode_and_stopped_budget() {
    let mut element = ParsedTextElement {
        fill_stroke_width: None,
        font_weight: None,
        baseline_y: Some(100.),
        page: 0,
        bounds: crate::ParsedBounds {
            x0: 20.,
            y0: 95.,
            x1: 100.,
            y1: 110.,
        },
        font_size: 12.,
        font_name: None,
        fill_color: None,
        text: "AAA α一😀".into(),
    };
    assert!(matches_preview(&budget(0), &[element.clone()], "AAA\nα\t一😀").unwrap());
    for different in ["AA α一😀", "AAAA α一😀", "AAA 一α😀", "AAA α一�"] {
        assert!(!matches_preview(&budget(0), &[element.clone()], different).unwrap());
    }
    let stopped = budget(0);
    stopped.charge_metadata_workspace_bytes(1);
    assert!(matches_preview(&stopped, &[], "").is_err());
    assert!(matches_preview(&stopped, &[element.clone()], &element.text).is_err());
    element.text.clear();
    assert!(!matches_preview(&budget(0), &[element], "X").unwrap());
}

/// 描画されたAをToUnicodeがCRへ対応させても、空白として捨てる復元経路へ入らない。
#[test]
fn indexed_recovery_rejects_painted_controls_but_keeps_generated_lines_and_hyphens() {
    let source = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F 5 0 R >> >> >>".into(),
        stream("BT /F 12 Tf 20 100 Td (A) Tj ET"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode 6 0 R >>".into(),
        stream("1 beginbfchar\n<41> <000D>\nendbfchar"),
    ]);
    let path = std::path::Path::new("owned-painted-control.pdf");
    with_native_bytes(&source, |page| {
        let text = page.text().unwrap();
        let chars = characters(&budget(65536), &text).unwrap();
        assert!(
            chars
                .iter()
                .any(|c| c.unicode_value() == 13 && !c.is_generated().unwrap())
        );
        assert!(!index_preserves_controls(&budget(0), path, &chars).unwrap());
    });
    with_page(
        "BT /F 12 Tf 20 100 Td (well-) Tj 0 -20 Td (known) Tj ET",
        |text| {
            let chars = characters(&budget(65536), text).unwrap();
            assert!(index_preserves_controls(&budget(0), path, &chars).unwrap());
            let stopped = budget(0);
            stopped.charge_metadata_workspace_bytes(1);
            assert!(index_preserves_controls(&stopped, path, &chars).is_err());
        },
    );
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    /// 文字サイズ・原点を変えても、矩形の境界では文字indexを使い、本物のAAA反復を失わない。
    #[test]
    fn native_lowered_glyphs_preserve_real_repetitions(
        x in 10u16..50, y in 70u16..140, size in 8u16..18,
    ) {
        let (x, y, size) = (f32::from(x), f32::from(y), f32::from(size));
        let content = format!(
            "BT /F {size} Tf 1 0 0 1 {x} {y} Tm (AAA) Tj 1 0 0 1 {} {y} Tm (LaT) Tj 1 0 0 1 {} {} Tm (E) Tj 1 0 0 1 {} {y} Tm (X) Tj ET",
            x + size * 3., x + size * 4.5, y - size * 0.19583333, x + size * 5.,
        );
        with_page(&content, |text| {
            assert_eq!(text.all().chars().filter(|c| !c.is_whitespace()).collect::<String>(), "AAALaTEX");
        });
        let limits = ParseLimits::default();
        let parsed = crate::pdfium_backend::parse_pdf(
            std::path::Path::new("owned-repeated-lowered-glyphs.pdf"),
            &owned_text_pdf(&content), &limits, &DecodeBudget::new(&limits),
        ).unwrap();
        let actual = parsed.text_elements.iter().flat_map(|e| e.text.chars()).filter(|c| !c.is_whitespace()).collect::<String>();
        proptest::prop_assert_eq!(actual, "AAALaTEX");
    }
}
