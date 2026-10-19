//! simple Font の文字間隔・水平倍率・rise と後続描画の原点を固定する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// Courier または一対多 ToUnicode の Font を持つページ/Form を生成する。
fn parse(content: &str, in_form: bool, ligature: bool) -> ParsedDocument {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(if in_form { "/Fm Do" } else { content });
    objects[5] = stream("");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push(if ligature {
        "<< /Type /Font /Subtype /Type1 /BaseFont /Custom /FirstChar 65 /LastChar 65 /Widths [700] /ToUnicode 11 0 R >>".into()
    } else {
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into()
    });
    if ligature {
        objects.push(stream("1 beginbfchar\n<41> <00660069>\nendbfchar"));
    }
    parse_pdf_bytes("text-spacing.pdf", &pdf(&objects)).unwrap()
}

/// Tc/Tw は code 数に適用し、末尾の間隔は表示範囲でなく次の原点へ反映する。
#[test]
fn spacing_advances_origin_without_trailing_bounds() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 2 Tc 5 Tw 10 10 Td (A B) Tj /F 20 Tf (C) Tj ET",
            in_form,
            false,
        );
        assert_eq!(parsed.text_elements[0].bounds.x1, 37.);
        assert_eq!(parsed.text_elements[1].bounds.x0, 39.);
    }
}

/// Tz は glyph と間隔/TJ 数値調整を倍率変更し、Tm の位置は変更しない。
#[test]
fn horizontal_scaling_applies_to_glyph_spacing_and_tj() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 2 Tc 50 Tz 10 10 Td [(A) 200 (B)] TJ /F 20 Tf (C) Tj ET",
            in_form,
            false,
        );
        assert_eq!(parsed.text_elements[0].bounds.x0, 10.);
        assert_eq!(parsed.text_elements[0].bounds.x1, 16.);
        assert_eq!(parsed.text_elements[1].bounds.x0, 17.);
        assert_eq!(parsed.text_elements[1].bounds.x1, 23.);
    }
}

/// Ts は回転した文字縦軸に沿って描画だけを移動し、次の原点を変更しない。
#[test]
fn rise_moves_rotated_baseline_without_changing_advance() {
    let parsed = parse(
        "BT /F 10 Tf 3 Ts 0 1 -1 0 100 100 Tm (A) Tj 0 Ts /F 20 Tf (B) Tj ET",
        true,
        false,
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 97.);
    assert_eq!(parsed.text_elements[1].bounds.x1, 100.);
    assert_eq!(parsed.text_elements[1].bounds.y0, 106.);
}

/// 二重引用符は Tw/Tc を更新して改行し、その状態を後続の Tj へ残す。
#[test]
fn double_quote_sets_persistent_word_and_character_spacing() {
    let parsed = parse(
        "BT /F 10 Tf 12 TL 10 50 Td 5 2 (A B) \" /F 20 Tf (C) Tj ET",
        true,
        false,
    );
    assert_eq!(parsed.text_elements[0].bounds.y0, 38.);
    assert_eq!(parsed.text_elements[0].bounds.x1, 37.);
    assert_eq!(parsed.text_elements[1].bounds.x0, 39.);
}

/// q/Q は spacing/scale/rise を保存し、BT は文字状態を初期化しない。
#[test]
fn graphics_restore_and_text_object_preserve_text_state() {
    let parsed = parse(
        "2 Tc 5 Tw 50 Tz 3 Ts q 0 Tc 0 Tw 100 Tz 0 Ts Q BT /F 10 Tf 10 10 Td (A B) Tj ET BT /F 20 Tf 50 50 Td (C) Tj ET",
        true,
        false,
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 23.5);
    assert_eq!(parsed.text_elements[0].bounds.y0, 13.);
    assert_eq!(parsed.text_elements[1].bounds.x1, 56.);
    assert_eq!(parsed.text_elements[1].bounds.y0, 53.);
}

/// Unicode の fi に対応する一 glyph には Tc を一度だけ適用する。
#[test]
fn ligature_spacing_uses_encoded_character_count() {
    let parsed = parse(
        "BT /F 10 Tf 2 Tc 10 10 Td (A) Tj /F 20 Tf (A) Tj ET",
        true,
        true,
    );
    assert_eq!(parsed.text_elements[0].text, "fi");
    assert_eq!(parsed.text_elements[1].bounds.x0, 19.);
}

/// 負の Tc/Tz でも glyph 全体の外接範囲と負方向の advance を保持する。
#[test]
fn negative_spacing_and_scaling_keep_all_glyph_extents() {
    let parsed = parse(
        "BT /F 10 Tf -10 Tc -50 Tz 100 100 Td (AB) Tj /F 20 Tf (C) Tj ET",
        true,
        false,
    );
    assert_eq!(parsed.text_elements[0].bounds.x0, 97.);
    assert_eq!(parsed.text_elements[0].bounds.x1, 102.);
    assert_eq!(parsed.text_elements[1].bounds.x0, 98.);
    assert_eq!(parsed.text_elements[1].bounds.x1, 104.);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// Tj/TJ の分割は間隔を含めた glyph の位置と後続原点を変えない。
    #[test]
    fn splitting_show_preserves_spacing(count in 1usize..20, spacing in -3i32..8, scale in 25i32..151) {
        let prefix = format!("BT /F 10 Tf {spacing} Tc {scale} Tz 100 100 Td ");
        let suffix = " /F 20 Tf (Z) Tj ET";
        let whole = parse(&format!("{prefix}({}) Tj{suffix}", "A".repeat(count)), true, false);
        let expected_origin = 100. + count as f32 * (6. + spacing as f32) * scale as f32 / 100.;
        prop_assert!((whole.text_elements[1].bounds.x0 - expected_origin).abs() < 0.001);
        for split in ["(A) Tj ".repeat(count), format!("[{}] TJ", "(A) ".repeat(count))] {
            let fragments = parse(&format!("{prefix}{split}{suffix}"), true, false);
            prop_assert_eq!(whole.text_elements.len(), fragments.text_elements.len());
            for (a, b) in whole.text_elements.iter().zip(&fragments.text_elements) {
                prop_assert_eq!((&a.text, &a.font_name, a.font_size, &a.fill_color), (&b.text, &b.font_name, b.font_size, &b.fill_color));
                for (x, y) in [(a.bounds.x0, b.bounds.x0), (a.bounds.x1, b.bounds.x1), (a.bounds.y0, b.bounds.y0), (a.bounds.y1, b.bounds.y1)] {
                    prop_assert!((x-y).abs() < 0.001);
                }
            }
        }
    }
}

/// 幅表が無い simple Font でも Tc/Tw は Unicode でなく encoded code を数える。
#[test]
fn unknown_simple_metrics_keep_encoded_spacing_semantics() {
    for (mapping, text, expected) in [
        ("<41> <00660069>", "A", 22.),
        ("<41> <0020>\n<42> <0042>", "AB", 24.),
    ] {
        let mut objects = pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> >>");
        objects[4] = stream(&format!(
            "BT /F 10 Tf 2 Tc 20 Tw 10 10 Td ({text}) Tj /F 20 Tf (B) Tj ET"
        ));
        objects[5] = stream("");
        objects
            .push("<< /Type /Font /Subtype /Type1 /BaseFont /Custom /ToUnicode 11 0 R >>".into());
        objects.push(stream(&format!("2 beginbfchar\n{mapping}\nendbfchar")));
        let parsed = parse_pdf_bytes("unknown-font-spacing.pdf", &pdf(&objects)).unwrap();
        assert_eq!(parsed.text_elements[1].bounds.x0, expected);
    }
}
