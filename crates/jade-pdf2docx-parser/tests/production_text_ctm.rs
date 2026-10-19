//! 各文字描画時点の CTM と q/Q 復帰を公開 API で固定する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 同じ Courier の内容をページまたは Form に配置して解析する。
fn parse(content: &str, in_form: bool) -> ParsedDocument {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(if in_form { "/Fm Do" } else { content });
    objects[5] = stream("");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [-500 -500 1000 1000] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    parse_pdf_bytes("text-ctm.pdf", &pdf(&objects)).unwrap()
}

/// 同じ BT 内の cm は後続の位置・幅・文字サイズにだけ作用する。
#[test]
fn scaling_inside_text_object_applies_to_each_show() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 10 20 Td (A) Tj 2 0 0 3 5 7 cm (B) Tj ET",
            in_form,
        );
        assert_eq!(parsed.text_elements.len(), 2);
        let first = &parsed.text_elements[0];
        let second = &parsed.text_elements[1];
        assert_eq!(
            (
                &first.text[..],
                first.font_size,
                first.bounds.x0,
                first.bounds.y0
            ),
            ("A", 10., 10., 20.)
        );
        assert_eq!(
            (
                &second.text[..],
                second.font_size,
                second.bounds.x0,
                second.bounds.y0,
                second.bounds.x1
            ),
            ("B", 30., 37., 67., 49.)
        );
    }
}

/// CTM の平行移動で離れた文字を、文字行列だけを見て結合しない。
#[test]
fn translated_draws_are_separate_elements() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 10 20 Td (A) Tj 1 0 0 1 100 50 cm (B) Tj ET",
            in_form,
        );
        assert_eq!(parsed.text_elements.len(), 2);
        assert_eq!(
            (
                &parsed.text_elements[1].text[..],
                parsed.text_elements[1].bounds.x0,
                parsed.text_elements[1].bounds.y0
            ),
            ("B", 116., 70.)
        );
    }
}

/// q/Q は CTM・Tm・Tlm を同時に復帰し、ET 後にも CTM を残す。
#[test]
fn graphics_restore_restores_draw_and_line_matrices() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 12 TL 10 50 Td (A) Tj q 2 0 0 3 5 7 cm 0 -12 Td (B) Tj Q /F 20 Tf (C) Tj (D) ' ET BT /F 30 Tf 10 20 Td (E) Tj ET",
            in_form,
        );
        assert_eq!(parsed.text_elements.len(), 5);
        let expected = [
            ("A", 10., 50., 10.),
            ("B", 25., 121., 30.),
            ("C", 16., 50., 20.),
            ("D", 10., 38., 20.),
            ("E", 10., 20., 30.),
        ];
        for (element, (text, x, y, size)) in parsed.text_elements.iter().zip(expected) {
            assert_eq!(
                (
                    &element.text[..],
                    element.bounds.x0,
                    element.bounds.y0,
                    element.font_size
                ),
                (text, x, y, size)
            );
        }
    }
}

/// BT 前後の cm は合成し、Q は次の BT へ元の CTM を渡す。
#[test]
fn outer_matrix_composition_and_restore_are_preserved() {
    for in_form in [false, true] {
        let parsed = parse(
            "q 2 0 0 3 5 7 cm 1 0 0 1 11 13 cm BT /F 10 Tf 10 20 Td (A) Tj ET Q BT /F 20 Tf 10 20 Td (B) Tj ET",
            in_form,
        );
        assert_eq!(parsed.text_elements.len(), 2);
        assert_eq!(
            (
                parsed.text_elements[0].bounds.x0,
                parsed.text_elements[0].bounds.y0,
                parsed.text_elements[0].font_size
            ),
            (47., 106., 30.)
        );
        assert_eq!(
            (
                parsed.text_elements[1].bounds.x0,
                parsed.text_elements[1].bounds.y0,
                parsed.text_elements[1].font_size
            ),
            (10., 20., 20.)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 変更後の CTM を独立したアフィン座標式で確認し、q/Q 復帰も固定する。
    #[test]
    fn draw_ctm_matches_affine_model(sx in 1i32..5, sy in 1i32..5, dx in 30i32..100, dy in -10i32..20, in_form in any::<bool>()) {
        let content = format!("BT /F 10 Tf 10 20 Td (A) Tj q {sx} 0 0 {sy} {dx} {dy} cm (B) Tj Q /F 20 Tf (C) Tj ET");
        let parsed = parse(&content, in_form);
        prop_assert_eq!(parsed.text_elements.len(), 3);
        let b = &parsed.text_elements[1];
        prop_assert_eq!((b.bounds.x0, b.bounds.y0, b.bounds.x1, b.font_size), ((16 * sx + dx) as f32, (20 * sy + dy) as f32, (22 * sx + dx) as f32, (10 * sy) as f32));
        let c = &parsed.text_elements[2];
        prop_assert_eq!((&c.text[..], c.bounds.x0, c.bounds.y0, c.font_size), ("C", 16., 20., 20.));
    }
}

/// 不正な q の operand を無視し、Q で架空の保存状態へ戻らない。
#[test]
fn invalid_graphics_save_does_not_create_matrix_state() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 10 20 Td (A) Tj 1 q 2 0 0 2 0 0 cm /F 20 Tf (B) Tj Q /F 30 Tf (C) Tj ET",
            in_form,
        );
        assert_eq!(parsed.text_elements.len(), 3);
        assert_eq!(
            (
                parsed.text_elements[2].bounds.x0,
                parsed.text_elements[2].bounds.y0,
                parsed.text_elements[2].font_size
            ),
            (56., 40., 60.)
        );
    }
}

/// 非数値と余分な operand を含む cm を後半の6数値だけで採用しない。
#[test]
fn malformed_matrix_operands_do_not_transform_text() {
    for in_form in [false, true] {
        let parsed = parse(
            "BT /F 10 Tf 10 20 Td (A) Tj 999 (ignored) 2 0 0 2 0 0 cm /F 20 Tf (B) Tj ET",
            in_form,
        );
        assert_eq!(parsed.text_elements.len(), 2);
        assert_eq!(
            (
                parsed.text_elements[1].bounds.x0,
                parsed.text_elements[1].bounds.y0,
                parsed.text_elements[1].font_size
            ),
            (16., 20., 20.)
        );
    }
}
