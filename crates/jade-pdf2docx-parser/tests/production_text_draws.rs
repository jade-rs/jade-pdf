//! 複数の文字描画を位置・Font・可視性ごとに抽出する回帰。
use jade_pdf2docx_parser::{ParsedDocument, QualityIssue, parse_pdf_bytes};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 書式が異なる独立空白も、後段の行復元に必要な位置付き断片として残す。
#[test]
fn standalone_space_retains_its_geometry_and_color() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 10 10 Td (A) Tj 0 0 1 rg ( ) Tj 1 0 0 rg (B) Tj ET",
            "[0 0 100 100]",
            in_form,
        );
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|text| text.text.as_str())
                .collect::<Vec<_>>(),
            ["A", " ", "B"]
        );
        let space = &parsed.text_elements[1];
        assert_eq!(space.fill_color.as_deref(), Some("0000FF"));
        assert_eq!((space.bounds.x0, space.bounds.x1), (16., 22.));
        assert!(!jade_pdf2docx_parser::text_is_visible(&parsed, space));
    }
}

/// 同一 text object の命令を正式なページまたは Form で実行する。
fn parse(content: &str, bbox: &str, in_form: bool) -> ParsedDocument {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R /G 11 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(if in_form { "/Fm Do" } else { content });
    objects[5] = stream("");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox {bbox} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    parse_pdf_bytes("text-draws.pdf", &pdf(&objects)).unwrap()
}

/// 最後の Tm と文字列を全描画へ適用しない。
#[test]
fn separate_matrices_clip_each_show() {
    for content in [
        "BT /F 10 Tf 1 0 0 1 10 10 Tm (VISIBLE) Tj 1 0 0 1 150 10 Tm (HIDDEN) Tj ET",
        "BT /F 10 Tf 1 0 0 1 150 10 Tm (HIDDEN) Tj 1 0 0 1 10 10 Tm (VISIBLE) Tj ET",
    ] {
        let parsed = parse(content, "[0 0 100 100]", true);
        assert_eq!(parsed.text_elements.len(), 1);
        assert_eq!(parsed.text_elements[0].text, "VISIBLE");
        assert_eq!(parsed.text_elements[0].bounds.x0, 10.);
        assert!(!parsed.text_preview.contains("HIDDEN"));
        assert!(
            parsed
                .quality
                .issues
                .iter()
                .any(|i| matches!(i, QualityIssue::UnsupportedXObject { .. }))
        );
    }
}

/// 各描画に直前の Font・文字サイズ・色を適用する。
#[test]
fn font_and_color_are_selected_per_show() {
    let parsed = parse(
        "BT /F 10 Tf 1 0 0 rg 10 10 Td (A) Tj /G 20 Tf 0 0 1 rg 1 0 0 1 50 50 Tm (B) Tj ET",
        "[0 0 100 100]",
        true,
    );
    assert_eq!(parsed.text_elements.len(), 2);
    let a = &parsed.text_elements[0];
    let b = &parsed.text_elements[1];
    assert_eq!(
        (
            a.text.as_str(),
            a.font_name.as_deref(),
            a.font_size,
            a.fill_color.as_deref()
        ),
        ("A", Some("Courier"), 10., Some("FF0000"))
    );
    assert_eq!(
        (
            b.text.as_str(),
            b.font_name.as_deref(),
            b.font_size,
            b.fill_color.as_deref()
        ),
        ("B", Some("Helvetica"), 20., Some("0000FF"))
    );
}

/// Tj は描画幅だけ text matrix を進め、Td は line matrix を基準にする。
#[test]
fn shows_advance_and_td_uses_line_origin() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 10 10 Td (A) Tj (B) Tj 10 20 Td (C) Tj ET",
            "[0 0 100 100]",
            in_form,
        );
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|t| (t.text.as_str(), t.bounds.x0, t.bounds.y0))
                .collect::<Vec<_>>(),
            [("AB", 10., 10.), ("C", 20., 30.)]
        );
        assert_eq!(parsed.text_elements[0].bounds.x1, 22.);
    }
}

/// TJ の数値調整で領域外へ進んだ後続文字を落とす。
#[test]
fn tj_adjustments_affect_visibility() {
    let parsed = parse(
        "BT /F 10 Tf 10 10 Td [(A) -2000 (HIDDEN)] TJ ET",
        "[0 0 20 100]",
        true,
    );
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "A");
    assert!(!parsed.text_preview.contains("HIDDEN"));
}

/// Td の移動量は回転した line matrix の軸で解釈する。
#[test]
fn td_moves_in_rotated_text_space() {
    let parsed = parse(
        "BT /F 10 Tf 0 1 -1 0 100 100 Tm (A) Tj 10 20 Td (B) Tj ET",
        "[0 0 200 200]",
        true,
    );
    assert_eq!(parsed.text_elements.len(), 2);
    let b = &parsed.text_elements[1];
    assert_eq!(
        (b.bounds.x0, b.bounds.y0, b.bounds.x1, b.bounds.y1),
        (70., 110., 80., 116.)
    );
}

/// TD/T* と引用符による改行・文字描画を順番に実行する。
#[test]
fn leading_and_quote_operators_move_lines() {
    let parsed = parse(
        "BT /F 10 Tf 10 100 Td (A) Tj 0 -20 TD (B) Tj T* (C) Tj (D) ' 0 0 (E) \" ET",
        "[0 0 200 200]",
        true,
    );
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| (t.text.as_str(), t.bounds.x0, t.bounds.y0))
            .collect::<Vec<_>>(),
        [
            ("A", 10., 100.),
            ("B", 10., 80.),
            ("C", 10., 60.),
            ("D", 10., 40.),
            ("E", 10., 20.)
        ]
    );
}

/// 数値や operator に見える文字列・コメント・container は state を変更しない。
#[test]
fn fake_operators_do_not_move_text() {
    let parsed = parse(
        "BT /F 10 Tf 10 10 Td (Tm Td Tf) Tj % 100 100 Td\n << /Fake [100 100 Td] >> /Tag BDC (B) Tj EMC ET",
        "[0 0 200 200]",
        true,
    );
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "Tm Td TfB");
    assert_eq!(parsed.text_elements[0].bounds.x0, 10.);
    assert_eq!(parsed.text_elements[0].bounds.x1, 64.);
    assert_eq!(parsed.text_elements[0].bounds.y0, 10.);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
    /// 描画後の Td は直前の glyph advance を引き継がず、行原点から移動する。
    #[test]
    fn relative_moves_preserve_line_matrix(dx in 0u16..50, dy in 1u16..50) {
        let parsed = parse(&format!("BT /F 10 Tf 10 10 Td (FIRST) Tj {dx} {dy} Td (SECOND) Tj ET"), "[0 0 200 200]", true);
        prop_assert_eq!(parsed.text_elements.len(), 2);
        prop_assert_eq!((parsed.text_elements[1].bounds.x0, parsed.text_elements[1].bounds.y0), (10. + f32::from(dx), 10. + f32::from(dy)));
    }
}

/// 同じ TJ の可視断片は位置調整後に一つの読みやすい文字列へまとめる。
#[test]
fn tj_fragments_keep_one_readable_sequence() {
    let parsed = parse(
        "BT /F 10 Tf 10 10 Td [(A) 10 (B)] TJ ET",
        "[0 0 100 100]",
        true,
    );
    assert_eq!(parsed.text_elements.len(), 1);
    let text = &parsed.text_elements[0];
    assert_eq!(text.text, "AB");
    assert!((text.bounds.x1 - 21.9).abs() < 0.0001);
}

/// q/Q は Font・色と文字/行行列を退避し、後続描画へ復帰する。
#[test]
fn graphics_state_restores_font_color_and_matrices() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 1 0 0 rg 10 10 Td (A) Tj q /G 20 Tf 0 g 1 0 0 1 50 50 Tm (B) Tj Q (C) Tj 10 20 Td (D) Tj ET",
            "[0 0 200 200]",
            in_form,
        );
        assert_eq!(parsed.text_elements.len(), 4);
        let c = &parsed.text_elements[2];
        assert_eq!(
            (
                c.text.as_str(),
                c.font_name.as_deref(),
                c.font_size,
                c.fill_color.as_deref(),
                c.bounds.x0,
                c.bounds.y0
            ),
            ("C", Some("Courier"), 10., Some("FF0000"), 16., 10.)
        );
        assert_eq!(
            (
                parsed.text_elements[3].bounds.x0,
                parsed.text_elements[3].bounds.y0
            ),
            (20., 30.)
        );
    }
}

/// BT は行列だけを初期化し、q/Q で復帰した text state は保持する。
#[test]
fn graphics_state_restores_across_text_objects() {
    let parsed = parse(
        "/F 10 Tf 1 0 0 rg q /G 20 Tf 0 g BT 10 10 Td (B) Tj ET Q BT 20 20 Td (C) Tj ET",
        "[0 0 200 200]",
        true,
    );
    let c = &parsed.text_elements[1];
    assert_eq!(
        (
            c.text.as_str(),
            c.font_name.as_deref(),
            c.font_size,
            c.fill_color.as_deref()
        ),
        ("C", Some("Courier"), 10., Some("FF0000"))
    );
}

/// 領域外描画のadvanceを保持し、後続の空白と本文を正しい位置へ進める。
#[test]
fn omitted_draws_still_advance_text_matrix() {
    let parsed = parse(
        "BT /F 10 Tf -6 10 Td (A) Tj (  ) Tj (B) Tj ET",
        "[0 0 100 100]",
        true,
    );
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(
        (
            parsed.text_elements[0].text.as_str(),
            parsed.text_elements[0].bounds.x0
        ),
        ("  B", 0.)
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 18.);
}

/// text object 外の q は文字行列を保存せず、Q は描画後の文字原点を維持する。
#[test]
fn save_outside_text_object_does_not_reset_text_matrix() {
    let parsed = parse(
        "/F 10 Tf q BT 10 10 Td (A) Tj Q (B) Tj ET",
        "[0 0 100 100]",
        true,
    );
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "AB");
    assert_eq!(
        (
            parsed.text_elements[0].bounds.x1,
            parsed.text_elements[0].bounds.y0
        ),
        (22., 10.)
    );
}

/// 同じ行の近接 Td 描画は文字ごとの分割を本文へ持ち込まない。
#[test]
fn horizontal_td_fragments_keep_readable_text() {
    let parsed = parse(
        "BT /F 10 Tf 10 10 Td (A) Tj 6 0 Td (B) Tj 6 0 Td (C) Tj ET",
        "[0 0 100 100]",
        true,
    );
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "ABC");
    assert_eq!(
        (
            parsed.text_elements[0].bounds.x0,
            parsed.text_elements[0].bounds.x1
        ),
        (10., 28.)
    );
}
