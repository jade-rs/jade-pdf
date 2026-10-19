//! Rootなしページの間接幾何属性を公開APIと独立した境界モデルで固定する。
use jade_pdf2docx_parser::{
    ParseError, ParseLimits, ParsedDocument, parse_pdf_bytes, parse_pdf_bytes_with_limits,
};
use proptest::prelude::*;

/// 正確なstream長と定義済みFontを持つRootなしfixtureを作る。
fn pdf(page: &str, parent: &str, extra: &str) -> Vec<u8> {
    let content = "BT /F 10 Tf 40 50 Td (AB) Tj ET 40 50 m 60 50 l S";
    format!("%PDF-1.7\n1 0 obj << /Type /Page /Parent 2 0 R {page} /Resources << /Font << /F 4 0 R >> >> /Contents 3 0 R >> endobj\n2 0 obj << /Type /Pages {parent} >> endobj\n3 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n4 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Courier >> endobj\n{extra}\n%%EOF\n", content.len()).into_bytes()
}

/// 公開ページと本文・罫線の座標を直接属性の独立fixtureへ照合する。
fn assert_equivalent(actual: &ParsedDocument, expected: &ParsedDocument) {
    assert_eq!(actual.pages, expected.pages);
    assert_eq!(actual.text_elements, expected.text_elements);
    assert_eq!(actual.graphics, expected.graphics);
    assert_eq!(actual.text_preview, expected.text_preview);
}

/// 間接配列・各座標・回転と多段参照を寸法・本文・罫線へ反映する。
#[test]
fn indirect_geometry_preserves_visible_coordinates() {
    let direct = parse_pdf_bytes(
        "direct.pdf",
        &pdf(
            "/MediaBox [0 0 100 200] /CropBox [10 20 90 170] /Rotate 90",
            "",
            "",
        ),
    )
    .unwrap();
    assert_eq!(
        (
            direct.pages[0].width,
            direct.pages[0].height,
            direct.pages[0].rotation
        ),
        (80.0, 150.0, 90)
    );
    assert_eq!(direct.text_preview, "AB");
    for (page, parent) in [
        ("/MediaBox 8 7 R /CropBox 9 3 R /Rotate 10 1 R", ""),
        ("", "/MediaBox 8 7 R /CropBox 9 3 R /Rotate 10 1 R"),
    ] {
        let extra = "8 7 obj 15 0 R endobj\n15 0 obj [0 0 100 200] endobj\n9 3 obj [11 7 R 12 3 R 13 0 R 14 0 R] endobj\n10 1 obj 16 0 R endobj\n16 0 obj 90 endobj\n11 7 obj 10 endobj\n12 3 obj 20 endobj\n13 0 obj 90 endobj\n14 0 obj 170 endobj";
        let indirect = parse_pdf_bytes("indirect.pdf", &pdf(page, parent, extra)).unwrap();
        assert_equivalent(&indirect, &direct);
    }
}

/// 直接・間接nullは最寄りの親から継承し、遠い祖先とは混ぜない。
#[test]
fn null_geometry_inherits_nearest_values() {
    let parent = "/MediaBox [0 0 100 200] /CropBox [10 20 90 170] /Rotate 180 /Parent 7 0 R";
    let direct = parse_pdf_bytes(
        "direct.pdf",
        &pdf(
            "",
            parent,
            "7 0 obj << /MediaBox [0 0 5 5] /CropBox [0 0 4 4] /Rotate 90 >> endobj",
        ),
    )
    .unwrap();
    for null in ["null", "8 7 R"] {
        let page = format!("/MediaBox {null} /CropBox {null} /Rotate {null}");
        let actual = parse_pdf_bytes("null.pdf", &pdf(&page, parent, "8 7 obj 9 3 R endobj\n9 3 obj null endobj\n7 0 obj << /MediaBox [0 0 5 5] /CropBox [0 0 4 4] /Rotate 90 >> endobj")).unwrap();
        assert_equivalent(&actual, &direct);
    }
}

/// 不正な直接幾何値は正常な親属性へfallbackして成功させない。
#[test]
fn invalid_geometry_values_are_rejected() {
    for (name, values) in [
        (
            "MediaBox",
            vec![
                "[]",
                "[0 0 100]",
                "[0 0 100 200 1]",
                "42",
                "[0 0 NaN 200]",
                "[0 0 1e2 200]",
                "[0 0 0 200]",
                "[-300000000000000000000000000000000000000 0 300000000000000000000000000000000000000 200]",
                "[0 0 999999999999999999999999999999999999999999999999999999 200]",
            ],
        ),
        (
            "CropBox",
            vec![
                "[]",
                "[0 0 100]",
                "42",
                "[0 0 NaN 200]",
                "[0 0 0 200]",
                "[300 300 400 400]",
            ],
        ),
        (
            "Rotate",
            vec!["45", "90.5", "90 180", "/Name", "[]", "2147483648", "1e2"],
        ),
    ] {
        for value in values {
            let page = format!("/{name} {value}");
            let result = parse_pdf_bytes(
                "bad-geometry.pdf",
                &pdf(
                    &page,
                    "/MediaBox [0 0 100 200] /CropBox [10 20 90 170] /Rotate 90",
                    "",
                ),
            );
            assert!(
                matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
                "{name}: {value}: {result:?}"
            );
        }
    }
}

/// 欠落・世代不一致・循環・不正最終値を黙った親継承で隠さない。
#[test]
fn invalid_geometry_references_are_rejected() {
    for name in ["MediaBox", "CropBox", "Rotate"] {
        for (reference, extra) in [
            ("99 0 R", ""),
            ("8 0 R", "8 7 obj null endobj"),
            ("8 7 R", "8 7 obj 9 3 R endobj\n9 3 obj 8 7 R endobj"),
            ("8 7 R", "8 7 obj << >> endobj"),
        ] {
            let result = parse_pdf_bytes(
                "bad-reference.pdf",
                &pdf(
                    &format!("/{name} {reference}"),
                    "/MediaBox [0 0 100 200] /CropBox [10 20 90 170] /Rotate 90",
                    extra,
                ),
            );
            assert!(
                matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
                "{name}: {reference}"
            );
        }
    }
}

/// 配列内の数値参照にも世代・循環・数値型検査を適用する。
#[test]
fn invalid_coordinate_references_are_rejected() {
    for extra in [
        "",
        "8 7 obj 100 endobj",
        "8 0 obj 8 0 R endobj",
        "8 0 obj (100) endobj",
    ] {
        let result = parse_pdf_bytes(
            "bad-coordinate.pdf",
            &pdf(
                "/MediaBox [0 0 8 0 R 200]",
                "/MediaBox [0 0 100 200]",
                extra,
            ),
        );
        assert!(matches!(
            result,
            Err(ParseError::InvalidPdfStructure { .. })
        ));
    }
}

/// 間接属性の参照深度はinclusiveで、一つ超えたら部分ページを返さない。
#[test]
fn geometry_reference_depth_is_inclusive() {
    for (name, value) in [
        ("MediaBox", "[0 0 100 200]"),
        ("CropBox", "[10 20 90 170]"),
        ("Rotate", "90"),
    ] {
        let bytes = pdf(
            &format!("/{name} 8 7 R"),
            "/MediaBox [0 0 100 200]",
            &format!(
                "8 7 obj 9 3 R endobj\n9 3 obj 10 0 R endobj\n10 0 obj 11 0 R endobj\n11 0 obj {value} endobj"
            ),
        );
        let mut limits = ParseLimits {
            max_container_nesting: 3,
            ..Default::default()
        };
        assert!(matches!(
            parse_pdf_bytes_with_limits("depth.pdf", &bytes, &limits),
            Err(ParseError::ResourceLimit {
                resource: "page attribute depth",
                limit: 3,
                actual: 4
            })
        ));
        limits.max_container_nesting = 4;
        assert!(parse_pdf_bytes_with_limits("depth.pdf", &bytes, &limits).is_ok());
    }
}

/// 自身のResourcesが空でも幾何継承に必要な親参照の破損を検出する。
#[test]
fn geometry_parent_reference_is_checked() {
    let bytes = String::from_utf8(pdf("/MediaBox [0 0 100 200]", "", ""))
        .unwrap()
        .replace("/Parent 2 0 R", "/Parent 99 0 R");
    assert!(matches!(
        parse_pdf_bytes("missing-parent.pdf", bytes.as_bytes()),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 後勝ちobjectの世代一致を三属性で独立した整数モデルと照合する。
    #[test]
    fn geometry_generation_matches_latest_object(reference in 0u16..10, actual in 0u16..10, attribute in 0usize..3) {
        let (name, value) = [("MediaBox", "[0 0 100 200]"), ("CropBox", "[10 20 90 170]"), ("Rotate", "90")][attribute];
        let bytes = pdf(&format!("/{name} 8 {reference} R"), "/MediaBox [0 0 100 200]", &format!("8 12 obj << >> endobj\n8 {actual} obj {value} endobj"));
        let result = parse_pdf_bytes("generation.pdf", &bytes);
        if reference == actual {
            let parsed = result.unwrap();
            let dimensions = if attribute == 1 { (80.0, 150.0) } else { (100.0, 200.0) };
            prop_assert_eq!((parsed.pages[0].width, parsed.pages[0].height), dimensions);
            prop_assert_eq!(parsed.pages[0].rotation, if attribute == 2 { 90 } else { 0 });
        } else {
            prop_assert!(matches!(result, Err(ParseError::InvalidPdfStructure { .. })), "幾何属性の世代不一致を拒否");
        }
    }

    /// 座標原点・四半回転・寸法を変えても間接化で公開結果を変えない。
    #[test]
    fn indirect_geometry_matches_direct_model(x in -100i16..100, y in -100i16..100, width in 80u16..300, height in 100u16..400, quarter in -4i32..5) {
        let right = i32::from(x) + i32::from(width);
        let top = i32::from(y) + i32::from(height);
        let direct = parse_pdf_bytes("direct.pdf", &pdf(&format!("/MediaBox [{x} {y} {right} {top}] /Rotate {}", quarter * 90), "", "")).unwrap();
        let actual = parse_pdf_bytes("indirect.pdf", &pdf("/MediaBox 8 7 R /Rotate 9 3 R", "", &format!("8 7 obj [{x} {y} {right} {top}] endobj\n9 3 obj {} endobj", quarter * 90))).unwrap();
        prop_assert_eq!((direct.pages[0].width, direct.pages[0].height), (f32::from(width), f32::from(height)));
        assert_equivalent(&actual, &direct);
    }
}
