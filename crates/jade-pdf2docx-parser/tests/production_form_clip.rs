//! Form の BBox と親・ページ領域による本文の可視判定を固定する。
use jade_pdf2docx_parser::{
    ParsedDocument, QualityIssue, UnsupportedXObjectReason, parse_pdf_bytes,
};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// BBox と本文を差し替えられる Form を作る。
fn form(attributes: &str, content: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form {attributes} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// 親から Font と子 Form を参照する正式な PDF を作る。
fn objects(attributes: &str, content: &str, page_content: &str) -> Vec<String> {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R /Child 11 0 R >> >>",
    );
    objects[4] = stream(page_content);
    objects[5] = stream("");
    objects[6] = form(attributes, content);
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects.push(form(
        "/BBox [0 0 100 100]",
        "BT /F 12 Tf 20 20 Td (CHILD) Tj ET",
    ));
    objects
}

/// pure Rust の通常入口で解析する。
fn parse(objects: &[String]) -> ParsedDocument {
    parse_pdf_bytes("form-clip.pdf", &pdf(objects)).unwrap()
}

/// 可視性にかかわらず実行した Form の品質警告を要求する。
fn assert_form_diagnostic(parsed: &ParsedDocument) {
    assert!(parsed.quality.issues.iter().any(|issue| matches!(
        issue,
        QualityIssue::UnsupportedXObject {
            page: 0,
            object_number: 7,
            reason: UnsupportedXObjectReason::Form
        }
    )));
}

/// 空・領域外・接するだけの本文は preview にも混入させない。
#[test]
fn invisible_bbox_excludes_text_and_preserves_diagnostics() {
    for bbox in [
        "[0 0 0 100]",
        "[0 0 0 0]",
        "[100 100 200 200]",
        "[0 0 20 100]",
    ] {
        let parsed = parse(&objects(
            &format!("/BBox {bbox}"),
            "BT /F 12 Tf 20 20 Td (FORM) Tj ET",
            "/Fm Do",
        ));
        assert!(
            parsed.text_elements.is_empty(),
            "{bbox}: {:?}",
            parsed.text_elements
        );
        assert!(!parsed.text_preview.contains("FORM"), "{bbox}");
        assert_form_diagnostic(&parsed);
    }
}

/// 部分交差では可視glyphだけを保持し、座標を可視領域へ制限する。
#[test]
fn partial_and_reversed_bbox_clip_bounds() {
    for bbox in ["[25 22 30 28]", "[30 28 25 22]"] {
        let parsed = parse(&objects(
            &format!("/BBox {bbox}"),
            "BT /F 12 Tf 20 20 Td (FORM) Tj ET",
            "/Fm Do",
        ));
        let text = &parsed.text_elements[0];
        // Helvetica 12ptではFが20..27.332、Oが27.332..36.668に交差する。
        assert_eq!(text.text, "FO");
        assert_eq!(
            (
                text.bounds.x0,
                text.bounds.y0,
                text.bounds.x1,
                text.bounds.y1
            ),
            (25.0, 22.0, 30.0, 28.0)
        );
        assert_form_diagnostic(&parsed);
    }
}

/// 子 BBox は親と交差し、復帰後の兄弟呼び出しには影響しない。
#[test]
fn nested_clip_and_sibling_restore() {
    let parsed = parse(&objects(
        "/BBox [0 0 100 100]",
        "q 1 0 0 1 150 0 cm /Child Do Q",
        "/Fm Do /Child Do",
    ));
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].bounds.x0, 20.0);
    assert_eq!(parsed.text_preview.matches("CHILD").count(), 1);
}

/// BBox は呼び出し時に固定し、本文中の cm では移動しない。
#[test]
fn internal_cm_does_not_move_bbox() {
    let parsed = parse(&objects(
        "/BBox [0 0 100 100]",
        "q 1 0 0 1 150 0 cm BT /F 12 Tf 20 20 Td (HIDDEN) Tj ET Q BT /F 12 Tf 20 20 Td (VISIBLE) Tj ET",
        "/Fm Do",
    ));
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "VISIBLE");
    assert!(!parsed.text_preview.contains("HIDDEN"));
}

/// 回転 BBox の外接矩形の隅を可視と誤判定しない。
#[test]
fn rotated_bbox_uses_polygon_not_envelope() {
    let parsed = parse(&objects(
        "/BBox [0 0 10 10] /Matrix [1 1 -1 1 100 100]",
        "q 0.5 -0.5 0.5 0.5 -100 0 cm BT /F 2 Tf 90 117 Td (X) Tj ET Q",
        "/Fm Do",
    ));
    assert!(
        parsed.text_elements.is_empty(),
        "{:?}",
        parsed.text_elements
    );
}

/// ページ領域も preview 組み立て前に適用する。
#[test]
fn page_media_and_crop_boxes_clip_preview() {
    for attributes in [
        "/MediaBox [0 0 10 10]",
        "/MediaBox [0 0 500 500] /CropBox [0 0 10 10]",
    ] {
        let mut objects = objects(
            "/BBox [0 0 100 100]",
            "BT /F 12 Tf 20 20 Td (HIDDEN) Tj ET",
            "/Fm Do",
        );
        objects[1] = objects[1].replace("/MediaBox [0 0 500 500]", attributes);
        let parsed = parse(&objects);
        assert!(parsed.text_elements.is_empty());
        assert!(!parsed.text_preview.contains("HIDDEN"));
    }
}

/// 面積を持たない変換では BBox の外接矩形に面積があっても不可視にする。
#[test]
fn singular_matrix_has_no_visible_area() {
    let parsed = parse(&objects(
        "/BBox [0 0 100 100] /Matrix [1 1 1 1 0 0]",
        "BT /F 12 Tf 20 20 Td (HIDDEN) Tj ET",
        "/Fm Do",
    ));
    assert!(parsed.text_elements.is_empty());
    assert!(!parsed.text_preview.contains("HIDDEN"));
}

/// 反転した文字の右端だけを原点と扱い、可視本文を落とさない。
#[test]
fn reflected_text_remains_visible_in_its_bbox() {
    let parsed = parse(&objects(
        "/BBox [20 20 44 32] /Matrix [-1 0 0 1 100 0]",
        "BT /F 12 Tf 20 20 Td (FORM) Tj ET",
        "/Fm Do",
    ));
    assert_eq!(parsed.text_elements.len(), 1);
    let bounds = parsed.text_elements[0].bounds;
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
        (56., 20., 80., 32.)
    );
    assert_eq!(parsed.text_elements[0].text, "FOR");
    assert_form_diagnostic(&parsed);
}

/// Form の縦反転は通常ページの罫線互換補正を受けず、BBox 内に残る。
#[test]
fn vertically_reflected_form_uses_page_space_clip() {
    let parsed = parse(&objects(
        "/BBox [20 20 44 32] /Matrix [1 0 0 -1 0 100]",
        "BT /F 12 Tf 20 20 Td (FORM) Tj ET",
        "/Fm Do",
    ));
    assert_eq!(parsed.text_elements.len(), 1);
    let bounds = parsed.text_elements[0].bounds;
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
        (20., 68., 44., 80.)
    );
    assert_eq!(parsed.text_elements[0].text, "FOR");
    assert_form_diagnostic(&parsed);
}

/// 水平と垂直の倍率を文字矩形のそれぞれの軸へ適用する。
#[test]
fn anisotropic_scale_uses_horizontal_width() {
    let parsed = parse(&objects(
        "/BBox [0 0 100 100] /Matrix [2 0 0 3 0 0]",
        "BT /F 12 Tf 20 20 Td (FORM) Tj ET",
        "/Fm Do",
    ));
    let bounds = parsed.text_elements[0].bounds;
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
        (40., 60., 110.656, 96.)
    );
    assert_eq!(parsed.text_elements[0].font_size, 36.);
}

/// 斜交と任意角回転では局所文字矩形の四隅を変換する。
#[test]
fn shear_and_rotation_transform_all_text_corners() {
    for (matrix, expected) in [
        ("[1 0 1 1 0 0]", (40., 20., 80.8, 32.)),
        ("[1 1 -1 1 100 100]", (88., 140., 128.8, 180.8)),
    ] {
        let mut objects = objects(
            &format!("/BBox [0 0 100 100] /Matrix {matrix}"),
            "BT /F 12 Tf 20 20 Td (FORM) Tj ET",
            "/Fm Do",
        );
        // Courier の対照入力で局所幅28.8ptを固定し、変換だけを検証する。
        objects[9] = "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into();
        let parsed = parse(&objects);
        let bounds = parsed.text_elements[0].bounds;
        assert_eq!(
            (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
            expected,
            "{matrix}"
        );
        assert_form_diagnostic(&parsed);
    }
}

/// Tm 自体の反転も呼び出し CTM との合成後に bounds へ反映する。
#[test]
fn reflected_text_matrix_remains_visible() {
    let parsed = parse(&objects(
        "/BBox [76 20 100 32]",
        "BT /F 12 Tf -1 0 0 1 100 20 Tm (FORM) Tj ET",
        "/Fm Do",
    ));
    assert_eq!(parsed.text_elements.len(), 1);
    let bounds = parsed.text_elements[0].bounds;
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
        (76., 20., 100., 32.)
    );
}

/// 文字矩形の外接 bounds が BBox と重なっても、変換前から領域外なら除外する。
#[test]
fn rotated_text_envelope_does_not_create_false_visibility() {
    let parsed = parse(&objects(
        "/BBox [0 0 10 10] /Matrix [1 1 -1 1 100 100]",
        "BT /F 4 Tf 11 0 Td (HIDDEN) Tj ET",
        "/Fm Do",
    ));
    assert!(
        parsed.text_elements.is_empty(),
        "{:?}",
        parsed.text_elements
    );
    assert!(!parsed.text_preview.contains("HIDDEN"));
    assert_form_diagnostic(&parsed);
}

/// 通常のページ本文でも CTM と Tm の斜交を合成して寸法を求める。
#[test]
fn page_text_combines_inverse_shears() {
    let parsed = parse(&objects(
        "/BBox [0 0 100 100]",
        "",
        "q 1 0 2 1 100 100 cm BT /F 12 Tf 1 0 -2 1 20 20 Tm (PAGE) Tj ET Q",
    ));
    assert_eq!(parsed.text_elements.len(), 1);
    let text = &parsed.text_elements[0];
    assert_eq!(text.font_size, 12.);
    assert_eq!(
        (text.bounds.x0, text.bounds.y0, text.bounds.y1),
        (160., 120., 132.)
    );
    assert!((text.bounds.x1 - 193.348).abs() < 0.0001);
    assert!(parsed.text_preview.contains("PAGE"));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
    /// 親 CTM と Form Matrix を合成しても交差範囲と本文を保つ。
    #[test]
    fn translated_and_scaled_clip(x in 0u16..100, y in 0u16..100, scale in 1u16..4) {
        let parsed = parse(&objects("/BBox [25 22 30 28] /Matrix [1 0 0 1 3 4]", "BT /F 12 Tf 20 20 Td (FORM) Tj ET", &format!("q {scale} 0 0 {scale} {x} {y} cm /Fm Do Q")));
        prop_assert_eq!(parsed.text_elements.len(), 1);
        let bounds = parsed.text_elements[0].bounds;
        let (x, y, scale) = (f32::from(x), f32::from(y), f32::from(scale));
        prop_assert_eq!((bounds.x0, bounds.y0, bounds.x1, bounds.y1), (x + 28.0 * scale, y + 26.0 * scale, x + 33.0 * scale, y + 32.0 * scale));
        prop_assert_eq!(&parsed.text_elements[0].text, "FO");
    }

    /// 互いに打ち消す斜交変換では縦倍率を個別に掛けず、元の寸法を保つ。
    #[test]
    fn inverse_shears_preserve_text_size(shear in -3i16..4) {
        let parsed = parse(&objects(
            &format!("/BBox [-200 -200 200 200] /Matrix [1 0 {shear} 1 100 100]"),
            &format!("BT /F 12 Tf 1 0 {} 1 20 20 Tm (FORM) Tj ET", -shear),
            "/Fm Do",
        ));
        prop_assert_eq!(parsed.text_elements.len(), 1);
        let text = &parsed.text_elements[0];
        let x = 120. + 20. * f32::from(shear);
        prop_assert_eq!(text.font_size, 12.);
        prop_assert_eq!((text.bounds.x0, text.bounds.y0, text.bounds.x1, text.bounds.y1), (x, 120., x + 35.328, 132.));
    }

    /// 非特異な一般行列でも、軸の区間演算で求めた解析解と一致する。
    #[test]
    fn affine_bounds_match_interval_solution(a in -2i16..3, b in -2i16..3, c in -2i16..3, d in -2i16..3) {
        prop_assume!(a * d - b * c != 0);
        let mut objects = objects(
            &format!("/BBox [0 0 100 100] /Matrix [{a} {b} {c} {d} 200 200]"),
            "BT /F 12 Tf 20 20 Td (FORM) Tj ET", "/Fm Do",
        );
        objects[9] = "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into();
        let parsed = parse(&objects);
        prop_assert_eq!(parsed.text_elements.len(), 1);
        let text = &parsed.text_elements[0];
        let (a, b, c, d) = (f32::from(a), f32::from(b), f32::from(c), f32::from(d));
        let x = 200. + 20. * a + 20. * c;
        let y = 200. + 20. * b + 20. * d;
        let expected = (
            x + (28.8 * a).min(0.) + (12. * c).min(0.),
            y + (28.8 * b).min(0.) + (12. * d).min(0.),
            x + (28.8 * a).max(0.) + (12. * c).max(0.),
            y + (28.8 * b).max(0.) + (12. * d).max(0.),
        );
        for (actual, expected) in [text.bounds.x0, text.bounds.y0, text.bounds.x1, text.bounds.y1].into_iter().zip([expected.0, expected.1, expected.2, expected.3]) {
            prop_assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
        }
        prop_assert!(parsed.text_preview.contains("FORM"));
        assert_form_diagnostic(&parsed);
    }
}
