//! 正式な page tree の表示属性と Resources 継承を検証する。
use jade_pdf2docx_parser::{ParseError, parse_pdf_bytes};
use proptest::prelude::*;

#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 必須 MediaBox が全祖先にもなければ、ページの部分欠落ではなく構造エラーにする。
#[test]
fn missing_media_box_is_rejected() {
    assert!(matches!(
        parse_pdf_bytes("missing-box.pdf", &pdf(&pages(""))),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// MediaBox は有限な PDF 数値ちょうど四つを持つ正の面積の配列である。
#[test]
fn malformed_media_box_is_rejected_without_parent_fallback() {
    for value in [
        "[0 0 100]",
        "[0 0 100 100 999]",
        "[0 0 (bad) 100 100]",
        "[0 0 NaN 100]",
        "[0 0 1e2 100]",
        "[0 0 0 100]",
        "[0 0 1e100 100]",
    ] {
        let mut objects = pages("/MediaBox [0 0 200 200]");
        objects[2] = format!("<< /Type /Page /Parent 2 0 R /MediaBox {value} >>");
        assert!(
            matches!(
                parse_pdf_bytes("bad-box.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{value}"
        );
    }
}

/// 間接 MediaBox の継承と直接 override は、Kids 順のページ寸法を保持する。
#[test]
fn indirect_media_box_is_inherited_and_overridden() {
    let mut objects = pages("/MediaBox 10 0 R");
    objects.push("[+0 -0 123.5 234.25]".into());
    objects[2] = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 99 88] >>".into();
    let parsed = parse_pdf_bytes("indirect-box.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.pages.len(), parsed.page_count);
    assert_eq!(
        parsed
            .pages
            .iter()
            .map(|p| (p.width, p.height))
            .collect::<Vec<_>>(),
        [(99.0, 88.0), (123.5, 234.25)]
    );
}

/// MediaBox の間接参照は欠落・世代不一致・循環で古い値に fallback しない。
#[test]
fn invalid_media_box_reference_is_rejected() {
    for reference in ["99 0 R", "10 1 R", "10 0 R"] {
        let mut objects = pages(&format!("/MediaBox {reference}"));
        objects.push(
            if reference == "10 0 R" {
                "10 0 R"
            } else {
                "[0 0 100 100]"
            }
            .into(),
        );
        assert!(
            matches!(
                parse_pdf_bytes("box-reference.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{reference}"
        );
    }
}

/// 親 Pages の直接・間接 Resources と間接 XObject 辞書から画像を描画する。
#[test]
fn inherited_resources_preserve_image_placements() {
    for resources in ["<< /XObject << /Im 7 0 R >> >>", "8 0 R"] {
        let objects = pages(&format!("/MediaBox [0 0 100 100] /Resources {resources}"));
        let parsed = parse_pdf_bytes("inherited-resources.pdf", &pdf(&objects)).unwrap();
        assert_eq!(parsed.image_elements.len(), 2, "{resources}");
        for (page, image) in parsed.image_elements.iter().enumerate() {
            assert_eq!(image.page, page);
            assert_eq!(
                image.bounds.x1 - image.bounds.x0,
                if page == 0 { 10.0 } else { 20.0 }
            );
        }
    }
}

/// Page の空 Resources は親の辞書を全体として上書きする。
#[test]
fn empty_resources_override_parent() {
    let mut objects = pages("/MediaBox [0 0 100 100] /Resources << /XObject << /Im 7 0 R >> >>");
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << >> >>".into();
    let parsed = parse_pdf_bytes("empty-resources.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.image_elements.len(), 1);
    assert_eq!(parsed.image_elements[0].page, 1);
}

/// Resources と XObject 辞書の欠落・世代不一致を画像の部分欠落で隠さない。
#[test]
fn invalid_resource_dictionary_reference_is_rejected() {
    for resources in ["99 0 R", "8 1 R", "[0]", "<< /XObject 9 1 R >>"] {
        let objects = pages(&format!("/MediaBox [0 0 100 100] /Resources {resources}"));
        let result = parse_pdf_bytes("bad-resources.pdf", &pdf(&objects));
        assert!(
            matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
            "{resources}: {}",
            match &result {
                Err(error) => format!("{error:?}"),
                Ok(_) => "unexpected success".into(),
            }
        );
    }
}

/// null の属性値は辞書エントリの不在と同じで、親の属性を継承する。
#[test]
fn null_attributes_inherit_parent() {
    let mut objects = pages("/MediaBox [0 0 100 100] /Resources << /XObject << /Im 7 0 R >> >>");
    objects[2] =
        "<< /Type /Page /Parent 2 0 R /MediaBox null /Resources null /Contents 5 0 R >>".into();
    let parsed = parse_pdf_bytes("null-attributes.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.pages.len(), 2);
    assert_eq!(parsed.image_elements.len(), 2);
}

/// XObject 辞書の参照 chain も、検証と描画で同じ最終辞書を使う。
#[test]
fn chained_xobject_dictionary_preserves_images() {
    let mut objects = pages("/MediaBox [0 0 100 100] /Resources 8 0 R");
    objects[8] = "10 0 R".into();
    objects.push("<< /Im 7 0 R >>".into());
    let parsed = parse_pdf_bytes("xobject-chain.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.image_elements.len(), 2);
}

/// XObject の値は stream であり、入れ子の辞書を黙って無視しない。
#[test]
fn nested_xobject_resource_is_rejected() {
    let objects =
        pages("/MediaBox [0 0 100 100] /Resources << /XObject << /Other << /Im 7 0 R >> >> >>");
    assert!(matches!(
        parse_pdf_bytes("nested-resource.pdf", &pdf(&objects)),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// 親に有効な寸法があっても、子の不正参照を null に変えて継承しない。
#[test]
fn invalid_child_box_reference_does_not_inherit() {
    let mut objects = pages("/MediaBox [0 0 100 100]");
    objects[2] = "<< /Type /Page /Parent 2 0 R /MediaBox 99 0 R >>".into();
    assert!(matches!(
        parse_pdf_bytes("child-reference.pdf", &pdf(&objects)),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// 間接 null も属性の不在として処理し、親の寸法を保持する。
#[test]
fn indirect_null_inherits_parent_box() {
    let mut objects = pages("/MediaBox [0 0 100 100]");
    objects[2] = "<< /Type /Page /Parent 2 0 R /MediaBox 10 0 R >>".into();
    objects.push("null".into());
    let parsed = parse_pdf_bytes("indirect-null.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.pages.len(), 2);
    assert_eq!(parsed.pages[0].width, 100.0);
}

/// 間接 array 要素と PDF コメントを含む MediaBox を数値として解決する。
#[test]
fn indirect_media_box_numbers_preserve_dimensions() {
    let mut objects = pages("/MediaBox [0 0 10 0 R % width\n .5]");
    objects.push("+.25".into());
    let parsed = parse_pdf_bytes("box-numbers.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.pages.len(), 2);
    assert_eq!((parsed.pages[0].width, parsed.pages[0].height), (0.25, 0.5));
}

/// MediaBox の間接参照深度は inclusive な上限で制限する。
#[test]
fn media_box_reference_depth_is_bounded() {
    use jade_pdf2docx_parser::{ParseLimits, parse_pdf_bytes_with_limits};
    let mut objects = pages("/MediaBox 10 0 R");
    objects.extend(["11 0 R".into(), "12 0 R".into(), "[0 0 100 100]".into()]);
    let limits = ParseLimits {
        max_container_nesting: 2,
        ..Default::default()
    };
    let bytes = pdf(&objects);
    assert!(matches!(
        parse_pdf_bytes_with_limits("box-depth.pdf", &bytes, &limits),
        Err(ParseError::ResourceLimit {
            resource: "page attribute depth",
            actual: 3,
            limit: 2
        })
    ));
    let limits = ParseLimits {
        max_container_nesting: 3,
        ..Default::default()
    };
    assert_eq!(
        parse_pdf_bytes_with_limits("box-depth.pdf", &bytes, &limits)
            .unwrap()
            .pages
            .len(),
        2
    );
}

/// 間接 CropBox/Rotate を親から継承し、各ページで直接値を上書きできる。
#[test]
fn indirect_crop_and_rotation_are_inherited_and_overridden() {
    let mut objects = pages("/MediaBox [0 0 100 100] /CropBox 10 0 R /Rotate 11 0 R");
    objects.extend(["[10 20 90 80]".into(), "90".into()]);
    objects[2] = "<< /Type /Page /Parent 2 0 R /CropBox [0 0 50 40] /Rotate 180 >>".into();
    let parsed = parse_pdf_bytes("indirect-viewport.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        (
            parsed.pages[0].width,
            parsed.pages[0].height,
            parsed.pages[0].rotation
        ),
        (50.0, 40.0, 180)
    );
    assert_eq!(
        (
            parsed.pages[1].width,
            parsed.pages[1].height,
            parsed.pages[1].rotation
        ),
        (80.0, 60.0, 90)
    );
}

/// 不正 CropBox を親の値や MediaBox へ黙って fallback しない。
#[test]
fn invalid_crop_box_is_rejected() {
    for value in [
        "[0 0 20]",
        "[0 0 20 20 30]",
        "[0 0 (bad) 20 20]",
        "[0 0 NaN 20]",
        "[0 0 0 20]",
        "[0 0 1e2 20]",
        "99 0 R",
        "10 1 R",
        "10 0 R",
    ] {
        let mut objects = pages("/MediaBox [0 0 100 100] /CropBox [0 0 50 50]");
        objects[2] = format!("<< /Type /Page /Parent 2 0 R /CropBox {value} >>");
        objects.push("10 0 R".into());
        assert!(
            matches!(
                parse_pdf_bytes("invalid-crop.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{value}"
        );
    }
}

/// Rotate は PDF 整数で90度単位であり、欠落・世代・循環も診断する。
#[test]
fn invalid_rotation_is_rejected() {
    for value in [
        "45",
        "90.0",
        "(90)",
        "/Right",
        "1e2",
        "2147483648",
        "99 0 R",
        "10 1 R",
        "10 0 R",
    ] {
        let mut objects = pages("/MediaBox [0 0 100 100] /Rotate 90");
        objects[2] = format!("<< /Type /Page /Parent 2 0 R /Rotate {value} >>");
        objects.push("10 0 R".into());
        assert!(
            matches!(
                parse_pdf_bytes("invalid-rotation.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{value}"
        );
    }
}

/// 符号付き整数の間接 Rotate は整数精度を保って解析する。
#[test]
fn signed_indirect_rotation_is_preserved() {
    for rotation in ["-90", "+450", "2147483610", "-2147483610"] {
        let mut objects = pages("/MediaBox [0 0 100 100] /Rotate 10 0 R");
        objects.push(rotation.into());
        let parsed = parse_pdf_bytes("signed-rotation.pdf", &pdf(&objects)).unwrap();
        assert_eq!(parsed.pages[0].rotation, rotation.parse::<i32>().unwrap());
    }
}

/// 間接 CropBox の配列要素も正規化した後で MediaBox と交差する。
#[test]
fn indirect_crop_box_components_preserve_intersection() {
    let mut objects = pages("/MediaBox [0 0 100 100] /CropBox [10 0 R 20 150 80]");
    objects.push("-50".into());
    let parsed = parse_pdf_bytes("indirect-crop-numbers.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        (parsed.pages[0].width, parsed.pages[0].height),
        (100.0, 60.0)
    );
}

/// 有効表示領域が空の CropBox を MediaBox に置き換えて描画しない。
#[test]
fn crop_box_with_empty_media_intersection_is_rejected() {
    for crop in ["[100 0 120 20]", "[110 110 120 120]"] {
        let objects = pages(&format!("/MediaBox [0 0 100 100] /CropBox {crop}"));
        assert!(matches!(
            parse_pdf_bytes("empty-crop.pdf", &pdf(&objects)),
            Err(ParseError::InvalidPdfStructure { .. })
        ));
    }
}

/// CropBox/Rotate の直接・間接 null は親の値を継承する。
#[test]
fn null_viewport_attributes_inherit_parent() {
    for value in ["null", "10 0 R"] {
        let mut objects = pages("/MediaBox [0 0 100 100] /CropBox [10 20 90 80] /Rotate -90");
        objects[2] = format!("<< /Type /Page /Parent 2 0 R /CropBox {value} /Rotate {value} >>");
        objects.push("null".into());
        let parsed = parse_pdf_bytes("null-viewport.pdf", &pdf(&objects)).unwrap();
        assert_eq!(
            (
                parsed.pages[0].width,
                parsed.pages[0].height,
                parsed.pages[0].rotation
            ),
            (80.0, 60.0, -90)
        );
    }
    let objects = pages("/MediaBox [0 0 100 100] /CropBox null /Rotate null");
    let parsed = parse_pdf_bytes("default-viewport.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        (
            parsed.pages[0].width,
            parsed.pages[0].height,
            parsed.pages[0].rotation
        ),
        (100.0, 100.0, 0)
    );
}

/// CropBox/Rotate の参照 chain に同じ inclusive 深度上限を適用する。
#[test]
fn viewport_reference_depth_is_bounded() {
    use jade_pdf2docx_parser::{ParseLimits, parse_pdf_bytes_with_limits};
    for (attribute, value) in [("CropBox", "[10 20 90 80]"), ("Rotate", "90")] {
        let mut objects = pages(&format!("/MediaBox [0 0 100 100] /{attribute} 10 0 R"));
        objects.extend(["11 0 R".into(), "12 0 R".into(), value.into()]);
        let bytes = pdf(&objects);
        let mut limits = ParseLimits {
            max_container_nesting: 2,
            ..Default::default()
        };
        assert!(matches!(
            parse_pdf_bytes_with_limits("viewport-depth.pdf", &bytes, &limits),
            Err(ParseError::ResourceLimit {
                resource: "page attribute depth",
                actual: 3,
                limit: 2
            })
        ));
        limits.max_container_nesting = 3;
        assert_eq!(
            parse_pdf_bytes_with_limits("viewport-depth.pdf", &bytes, &limits)
                .unwrap()
                .pages
                .len(),
            2
        );
    }
}

/// 間接表示属性による切り抜き・原点移動・回転を画像配置にも適用する。
#[test]
fn indirect_viewport_clips_and_rotates_image_placements() {
    let mut objects =
        pages("/MediaBox [0 0 100 100] /CropBox 10 0 R /Rotate 11 0 R /Resources 8 0 R");
    objects.extend(["[10 20 90 80]".into(), "90".into()]);
    objects[4] = stream("q 30 0 0 30 0 10 cm /Im Do Q");
    objects[5] = stream("q 5 0 0 5 0 0 cm /Im Do Q");
    let parsed = parse_pdf_bytes("viewport-image.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.image_elements.len(), 1);
    let image = &parsed.image_elements[0];
    assert_eq!(image.page, 0);
    assert_eq!(
        (
            image.bounds.x0,
            image.bounds.y0,
            image.bounds.x1,
            image.bounds.y1
        ),
        (0.0, 60.0, 20.0, 80.0)
    );
}

proptest! {
    /// 間接 Rotate は i32 範囲内の90度単位を整数精度のまま保持する。
    #[test]
    fn indirect_rotation_preserves_integer_multiples(quarter_turns in -23_860_929i32..=23_860_929) {
        let rotation = quarter_turns * 90;
        let mut objects = pages("/MediaBox [0 0 100 100] /Rotate 10 0 R");
        objects.push(format!("{rotation}"));
        let parsed = parse_pdf_bytes("rotation-property.pdf", &pdf(&objects)).unwrap();
        for page in parsed.pages { prop_assert_eq!(page.rotation, rotation); }
    }

    /// CropBox の向きと間接要素によらず MediaBox との交差寸法を保持する。
    #[test]
    fn crop_intersection_preserves_dimensions(
        left in -100i32..100, bottom in -100i32..100,
        right in 101i32..200, top in 101i32..200,
        reverse_x in any::<bool>(), reverse_y in any::<bool>(),
    ) {
        let (x0, x1) = if reverse_x { (right, left) } else { (left, right) };
        let (y0, y1) = if reverse_y { (top, bottom) } else { (bottom, top) };
        let mut objects = pages(&format!("/MediaBox [0 0 100 100] /CropBox [10 0 R {y0} {x1} {y1}]"));
        objects.push(format!("{x0}"));
        let parsed = parse_pdf_bytes("crop-property.pdf", &pdf(&objects)).unwrap();
        for page in parsed.pages {
            prop_assert_eq!((page.width, page.height), ((100 - left.max(0)) as f32, (100 - bottom.max(0)) as f32));
        }
    }

    /// 座標の符号と向きによらず正規化後の寸法を保持する。
    #[test]
    fn media_box_normalization_preserves_dimensions(
        x in -1000i32..1000,
        y in -1000i32..1000,
        width in 1i32..1000,
        height in 1i32..1000,
        reverse_x in any::<bool>(),
        reverse_y in any::<bool>(),
    ) {
        let (left, right) = if reverse_x { (x + width, x) } else { (x, x + width) };
        let (bottom, top) = if reverse_y { (y + height, y) } else { (y, y + height) };
        let objects = pages(&format!("/MediaBox [{left} {bottom} {right} {top}]"));
        let parsed = parse_pdf_bytes("normalized-box.pdf", &pdf(&objects)).unwrap();
        prop_assert_eq!(parsed.pages.len(), 2);
        for page in parsed.pages {
            prop_assert_eq!((page.width, page.height), (width as f32, height as f32));
        }
    }

    /// Resources の欠落/null は継承し、空辞書は親を上書きする。
    #[test]
    fn closest_resource_dictionary_controls_images(
        indirect in any::<bool>(),
        override_kind in 0u8..3,
    ) {
        let resources = if indirect { "8 0 R" } else { "<< /XObject << /Im 7 0 R >> >>" };
        let mut objects = pages(&format!("/MediaBox [0 0 100 100] /Resources {resources}"));
        let own = match override_kind { 0 => "", 1 => "/Resources null", _ => "/Resources << >>" };
        objects[2] = format!("<< /Type /Page /Parent 2 0 R /Contents 5 0 R {own} >>");
        let parsed = parse_pdf_bytes("closest-resources.pdf", &pdf(&objects)).unwrap();
        prop_assert_eq!(parsed.image_elements.len(), if override_kind == 2 { 1 } else { 2 });
    }
}
