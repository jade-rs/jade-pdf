//! 通常のpath clipによる非表示本文とgraphics stateの抽出回帰。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// ページと再利用するFormを正式PDFへ組み立てる。
fn objects(page: &str, form: &str) -> Vec<String> {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(page);
    objects[5] = stream("BT /F 10 Tf 20 20 Td (Next page) Tj ET");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] /Length {} >>\nstream\n{form}\nendstream",
        form.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    objects
}

/// 先頭ページの文字を描画順で比較する。
fn text(parsed: &ParsedDocument) -> String {
    parsed
        .text_elements
        .iter()
        .filter(|t| t.page == 0)
        .map(|t| t.text.as_str())
        .collect()
}

/// 同じTj内の可視glyphだけを保持し、隠れたglyphのadvanceも次のTjへ適用する。
#[test]
fn partial_clip_filters_glyphs_without_changing_advance() {
    for in_form in [false, true] {
        let content = "q 32 15 18 20 re W n BT /F 10 Tf 20 20 Td (XXABCYY) Tj ET Q BT /F 10 Tf 1 0 0 rg 20 60 Td (Next) Tj ET";
        let parsed = parse_pdf_bytes(
            "partial.pdf",
            &pdf(&objects(if in_form { "/Fm Do" } else { content }, content)),
        )
        .unwrap();
        assert_eq!(text(&parsed), "ABCNext", "form={in_form}");
        assert!(!parsed.text_preview.contains("XX"));
        let visible = parsed
            .text_elements
            .iter()
            .find(|text| text.page == 0)
            .unwrap();
        assert_eq!([visible.bounds.x0, visible.bounds.x1], [32., 50.]);
        let content =
            "q 32 15 200 20 re W n BT /F 10 Tf 20 20 Td (XXABC) Tj 1 0 0 rg (Next) Tj ET Q";
        let parsed = parse_pdf_bytes(
            "advance.pdf",
            &pdf(&objects(if in_form { "/Fm Do" } else { content }, content)),
        )
        .unwrap();
        assert_eq!(text(&parsed), "ABCNext");
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .filter(|text| text.page == 0)
                .nth(1)
                .unwrap()
                .bounds
                .x0,
            50.
        );
    }
}

/// native抽出でも単一text objectのclip外glyphを本文へ復活させない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_partial_clip_keeps_only_visible_glyphs() {
    for (label, page, form) in [
        (
            "page",
            "q 32 15 18 20 re W n BT /F 10 Tf 20 20 Td (XXABCYY) Tj ET Q",
            "",
        ),
        (
            "form",
            "q 2 0 0 2 100 100 cm /Fm Do Q",
            "q 32 15 18 20 re W n BT /F 10 Tf 20 20 Td (XXABCYY) Tj ET Q",
        ),
        (
            "caller",
            "q 164 130 36 40 re W n 2 0 0 2 100 100 cm /Fm Do Q",
            "BT /F 10 Tf 20 20 Td (XXABCYY) Tj ET",
        ),
    ] {
        let original = pdf(&objects(page, form));
        let path = std::env::temp_dir().join(format!(
            "jade-pdf2docx-partial-clip-{label}-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, &original).unwrap();
        let parsed = jade_pdf2docx_parser::parse_pdf(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
        assert_eq!(text(&parsed).trim(), "ABC", "{label}");
        assert!(
            !parsed.text_preview.contains("XX"),
            "{label}: {}",
            parsed.text_preview
        );
    }
}

/// 部分clipと別objectの重なりは、native重複排除で本文が欠ける前に拒否する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_partial_clip_rejects_overlapping_visible_object() {
    for clipped_first in [false, true] {
        for placement in ["page", "form", "transformed", "caller", "reused"] {
            let clipped = "q 50 15 6 20 re W n BT /F 10 Tf 20 20 Td (alphaA) Tj ET Q";
            let visible = "BT /F 10 Tf 20 20 Td (alpha) Tj ET";
            let (clipped, visible, form) = match placement {
                "transformed" => (
                    "q 2 0 0 2 100 100 cm /Fm Do Q".to_string(),
                    format!("q 2 0 0 2 100 100 cm {visible} Q"),
                    clipped.to_string(),
                ),
                "caller" | "reused" => (
                    "q 200 130 12 40 re W n 2 0 0 2 100 100 cm /Fm Do Q".to_string(),
                    if placement == "reused" {
                        "q 2 0 0 2 100 100 cm /Fm Do Q".to_string()
                    } else {
                        format!("q 2 0 0 2 100 100 cm {visible} Q")
                    },
                    "BT /F 10 Tf 20 20 Td (alphaA) Tj ET".to_string(),
                ),
                _ => (clipped.to_string(), visible.to_string(), String::new()),
            };
            let content = if clipped_first {
                format!("{clipped} {visible}")
            } else {
                format!("{visible} {clipped}")
            };
            let original = if placement == "form" {
                pdf(&objects("/Fm Do", &content))
            } else {
                pdf(&objects(&content, &form))
            };
            let path = std::env::temp_dir().join(format!(
                "jade-pdf2docx-overlap-partial-{clipped_first}-{placement}-{}.pdf",
                std::process::id()
            ));
            std::fs::write(&path, &original).unwrap();
            let result = jade_pdf2docx_parser::parse_pdf(&path);
            assert_eq!(std::fs::read(&path).unwrap(), original);
            std::fs::remove_file(path).unwrap();
            assert!(
                matches!(result, Err(jade_pdf2docx_parser::ParseError::Pdfium { message, .. })
                if message.contains("overlapping partially clipped text")),
                "first={clipped_first} placement={placement}"
            );
        }
    }
}

/// 局所座標が同じでも親Formの変換後に離れる本文は、部分clipと併存できる。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_partial_clip_accepts_separate_visible_object() {
    for page in [
        "q 2 0 0 2 100 100 cm /Fm Do Q BT /F 10 Tf 20 20 Td (alpha) Tj ET",
        "BT /F 10 Tf 20 20 Td (alpha) Tj ET q 2 0 0 2 100 100 cm /Fm Do Q",
    ] {
        let original = pdf(&objects(
            page,
            "q 50 15 6 20 re W n BT /F 10 Tf 20 20 Td (alphaA) Tj ET Q",
        ));
        let path = std::env::temp_dir().join(format!(
            "jade-pdf2docx-separated-partial-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, &original).unwrap();
        let parsed = jade_pdf2docx_parser::parse_pdf(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
        let body = text(&parsed);
        assert_eq!(body.matches("alpha").count(), 1, "{body:?}");
        assert_eq!(body.matches('A').count(), 1, "{body:?}");
    }
}

/// clipの可視側で隣接する別objectは、隠れたglyphとの重複として拒否しない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_partial_clip_accepts_adjacent_visible_object() {
    let original = pdf(&objects(
        "q 32 15 200 20 re W n BT /F 10 Tf 20 20 Td (XXABC) Tj 1 0 0 rg (Next) Tj ET Q",
        "",
    ));
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-adjacent-partial-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, &original).unwrap();
    let parsed = jade_pdf2docx_parser::parse_pdf(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(path).unwrap();
    assert_eq!(text(&parsed), "ABCNext");
}

/// 矩形の外側の本文を除き、Q復元・別ページ・Form内のclipを区別する。
#[test]
fn clipped_text_does_not_enter_body_or_preview() {
    for end in ["n", "S", "f", "F", "f*", "B", "B*", "s", "b", "b*"] {
        for in_form in [false, true] {
            let content = format!(
                "q 0 0 1 1 re W {end} BT /F 10 Tf 20 20 Td (Hidden) Tj ET Q BT /F 10 Tf 20 20 Td (Visible) Tj ET"
            );
            let parsed = parse_pdf_bytes(
                "clip.pdf",
                &pdf(&objects(
                    if in_form { "/Fm Do" } else { &content },
                    &content,
                )),
            )
            .unwrap();
            assert_eq!(text(&parsed), "Visible", "{end} form={in_form}");
            assert!(!parsed.text_preview.contains("Hidden"));
            assert!(parsed.text_preview.contains("Next page"));
        }
    }
}

/// Contents境界でCTM・現在path・pending clipを保持し、Qは現在pathを戻さない。
#[test]
fn contents_and_graphics_restore_keep_the_current_path() {
    let mut input = objects("q 1 0 0 1 100 100 cm 0 0 1 1 re", "");
    input[2] = input[2].replace("/Contents 5 0 R", "/Contents [5 0 R 11 0 R]");
    input.push(stream(
        "Q W n BT /F 10 Tf 20 20 Td (Hidden) Tj 1 0 0 1 100 100 Tm (Visible) Tj ET",
    ));
    let parsed = parse_pdf_bytes("contents-clip.pdf", &pdf(&input)).unwrap();
    assert_eq!(text(&parsed), "V");
    assert!(!parsed.quality.issues.iter().any(|issue| matches!(
        issue,
        jade_pdf2docx_parser::QualityIssue::UnsupportedClippingPath { .. }
    )));
}

/// 退化したpathは本文を隠し、未構築pathや文字列内の偽operatorは適用しない。
#[test]
fn empty_clip_and_fake_operators_are_distinct() {
    for (content, expected) in [
        ("W n BT /F 10 Tf 20 20 Td (Visible) Tj ET", "Visible"),
        ("0 0 m W n BT /F 10 Tf 20 20 Td (Hidden) Tj ET", ""),
        ("0 0 0 0 re W n BT /F 10 Tf 20 20 Td (Hidden) Tj ET", ""),
        (
            "(0 0 1 1 re W n) pop [0 0 1 1 re W n] pop BT /F 10 Tf 20 20 Td (Visible) Tj ET",
            "Visible",
        ),
        (
            "0 0 1 1 re W BT /F 10 Tf 20 20 Td (Visible) Tj ET n",
            "Visible",
        ),
    ] {
        let parsed = parse_pdf_bytes("empty-clip.pdf", &pdf(&objects(content, ""))).unwrap();
        assert_eq!(text(&parsed), expected);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config { cases: 32, failure_persistence: None, ..Default::default() })]
    /// clip区間・文字間隔・文字幅倍率を変えても、元codeの可視部分とadvanceが一致する。
    #[test]
    fn partial_glyph_interval_matches_independent_model(
        count in 3usize..18, start in 0usize..18, length in 1usize..18,
        spacing in 0u8..5, scale in 1u8..4,
    ) {
        let start = start % count;
        let end = (start + length).min(count);
        let text = &"ABCDEFGHIJKLMNOPQR"[..count];
        let step = (6. + f32::from(spacing)) * f32::from(scale);
        let left = 20. + start as f32 * step;
        let right = 20. + (end - 1) as f32 * step + 6. * f32::from(scale);
        let content = format!("q {left} 15 {} 20 re W n BT /F 10 Tf {spacing} Tc {} Tz 20 20 Td ({text}) Tj ET Q", right-left, scale as u16 * 100);
        let parsed = parse_pdf_bytes("generated-partial.pdf", &pdf(&objects(&content, ""))).unwrap();
        proptest::prop_assert_eq!(self::text(&parsed), &text[start..end]);
        let bounds = parsed.text_elements.iter().find(|text| text.page == 0).unwrap().bounds;
        proptest::prop_assert_eq!([bounds.x0, bounds.x1], [left, right]);
    }

    /// 平行移動・winding・even-odd指定を変えても、領域内外の分類が不変である。
    #[test]
    fn rectangle_visibility_is_invariant(
        x in 10u16..200, y in 10u16..200,
        reverse in proptest::bool::ANY, even_odd in proptest::bool::ANY,
    ) {
        let operator = if even_odd { "W*" } else { "W" };
        let path = if reverse { "80 80 -80 -80 re" } else { "0 0 80 80 re" };
        let content = format!("q 1 0 0 1 {x} {y} cm {path} {operator} n BT /F 10 Tf 10 10 Td (Inside) Tj 1 0 0 1 100 100 Tm (Outside) Tj ET Q");
        let parsed = parse_pdf_bytes("generated-clip.pdf", &pdf(&objects(&content, ""))).unwrap();
        proptest::prop_assert_eq!(text(&parsed), "Inside");
        proptest::prop_assert!(!parsed.quality.issues.iter().any(|issue| matches!(issue, jade_pdf2docx_parser::QualityIssue::UnsupportedClippingPath { .. })), "generated rectangle must be supported");
    }
}

/// 呼び出し元のclipをFormへ継承し、復元後の再利用では本文を残す。
#[test]
fn form_inherits_caller_path_clip() {
    let parsed = parse_pdf_bytes(
        "clip.pdf",
        &pdf(&objects(
            "q 0 0 1 1 re W* n /Fm Do Q /Fm Do",
            "BT /F 10 Tf 20 20 Td (Visible) Tj ET",
        )),
    )
    .unwrap();
    assert_eq!(text(&parsed), "Visible");
}

/// clipの頂点をpath構築時のCTMで固定し、凸三角形と部分交差を扱う。
#[test]
fn transformed_polygon_keeps_only_intersecting_text() {
    let content = "q 1 0 0 1 100 100 cm 0 0 m 40 0 l 0 40 l h W n 1 0 0 1 -100 -100 cm BT /F 10 Tf 125 125 Td (Hidden) Tj 1 0 0 1 100 100 Tm (Visible) Tj ET Q";
    let parsed = parse_pdf_bytes("clip.pdf", &pdf(&objects(content, ""))).unwrap();
    assert_eq!(text(&parsed), "Visible");
    let visible = parsed
        .text_elements
        .iter()
        .find(|t| t.text == "Visible")
        .unwrap();
    assert!(visible.bounds.x1 <= 140. && visible.bounds.y1 <= 140.);
}

/// 未対応の曲線・複合・非凸clipを可視性保証済みとして扱わない。
#[test]
fn unsupported_clip_is_diagnosed() {
    for path in [
        "0 0 m 10 0 10 10 0 10 c",
        "0 0 10 10 re 20 20 10 10 re",
        "0 0 m 20 0 l 5 5 l 20 20 l 0 20 l h",
    ] {
        let content = format!("{path} W n BT /F 10 Tf 20 20 Td (Probe) Tj ET");
        let parsed = parse_pdf_bytes("clip.pdf", &pdf(&objects(&content, ""))).unwrap();
        assert!(parsed.quality.issues.iter().any(|issue| matches!(
            issue,
            jade_pdf2docx_parser::QualityIssue::UnsupportedClippingPath { page: 0 }
        )));
    }
}

/// native抽出でもclipの外側の重複glyphを復元せず、元PDFを保持する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_clip_does_not_restore_hidden_overlapping_glyphs() {
    for (label, page, form) in [
        (
            "page",
            "q 0 0 1 1 re W n BT /F 10 Tf 20 20 Td (Hidden) Tj ET Q BT /F 10 Tf 20 20 Td (Visible) Tj ET",
            "",
        ),
        (
            "caller",
            "q 0 0 1 1 re W n /Fm Do Q /Fm Do",
            "BT /F 10 Tf 20 20 Td (Visible) Tj ET",
        ),
        (
            "form",
            "/Fm Do",
            "q 0 0 1 1 re W n BT /F 10 Tf 20 20 Td (Hidden) Tj ET Q BT /F 10 Tf 20 20 Td (Visible) Tj ET",
        ),
        (
            "transformed",
            "q 2 0 0 2 100 100 cm /Fm Do Q",
            "q 0 0 1 1 re W n BT /F 10 Tf 20 20 Td (Hidden) Tj ET Q BT /F 10 Tf 20 20 Td (Visible) Tj ET",
        ),
        (
            "parent-and-transform",
            "q 135 135 100 100 re W n 2 0 0 2 100 100 cm /Fm Do Q",
            "q 0 0 1 1 re W n BT /F 10 Tf 20 20 Td (Hidden) Tj ET Q BT /F 10 Tf 20 20 Td (Visible) Tj ET",
        ),
        (
            "degenerate",
            "q 0 0 m W n BT /F 10 Tf 20 20 Td (Hidden) Tj ET Q BT /F 10 Tf 20 20 Td (Visible) Tj ET",
            "",
        ),
        (
            "polygon",
            "q 0 0 m 40 0 l 0 40 l h W n BT /F 10 Tf 25 25 Td (Hidden) Tj ET Q BT /F 10 Tf 25 25 Td (Visible) Tj ET",
            "",
        ),
        (
            "intersection",
            "q 0 0 50 50 re W n 100 100 50 50 re W n BT /F 10 Tf 20 20 Td (Hidden) Tj ET Q BT /F 10 Tf 20 20 Td (Visible) Tj ET",
            "",
        ),
    ] {
        let original = pdf(&objects(page, form));
        let path = std::env::temp_dir().join(format!(
            "jade-pdf2docx-path-clip-{label}-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, &original).unwrap();
        let parsed = jade_pdf2docx_parser::parse_pdf(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
        assert_eq!(text(&parsed).trim(), "Visible", "{label}");
        assert!(!parsed.text_preview.contains("Hidden"));
    }
}
