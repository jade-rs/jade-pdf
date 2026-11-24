//! 入れ子・独立した状態・画素clip・容量上限の不変条件。
use super::*;
use proptest::prelude::*;

/// 同名の局所画像とgsを分離し、子の変更を呼出側へ漏らさない。
#[test]
fn nested_forms_resolve_local_names_and_restore_parent_paint() {
    let mut objects = objects("/Half gs /Fm Do", "", "");
    objects[4] = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Matrix [2 0 0 3 4 5] /Resources << /XObject << /Im 6 0 R /Child 7 0 R >> >>",
        b"/Im Do /Child Do /Im Do",
    );
    objects.push(stream("/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Matrix [1 0 0 1 2 3] /Resources << /XObject << /Im 8 0 R >> /ExtGState << /Half << /ca .25 >> >> >>", b"/Half gs /Im Do"));
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[0, 255, 0]).unwrap();
    objects.push(stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode", &encoder.finish().unwrap()));
    let parsed = parse_pdf_bytes("owned-nested-local-images.pdf", &document(&objects)).unwrap();
    assert_eq!(parsed.image_elements.len(), 3);
    let pixels: Vec<_> = parsed
        .image_elements
        .iter()
        .map(|image| {
            decode_filter0_png_to_rgba(&DecodeBudget::default(), &image.png_bytes)
                .unwrap()
                .rgba
        })
        .collect();
    assert_eq!(
        pixels,
        [[255, 0, 0, 128], [255, 0, 0, 128], [0, 255, 0, 64]]
    );
    assert_eq!(
        parsed.image_elements[2].bounds,
        ParsedBounds {
            x0: 8.,
            y0: 14.,
            x1: 10.,
            y1: 17.
        }
    );
}

/// 呼ばれないFormと完全にclip外の画像を成果物へ混ぜない。
#[test]
fn unused_and_fully_clipped_images_are_not_extracted() {
    for (page, body) in [("", "/Im Do"), ("/Fm Do", "10 0 0 10 20 20 cm /Im Do")] {
        let parsed = parse_pdf_bytes("owned-unpainted-form.pdf", &fixture(page, body, "")).unwrap();
        assert!(parsed.image_elements.is_empty());
        assert!(
            !parsed
                .quality
                .issues
                .iter()
                .any(|issue| matches!(issue, QualityIssue::OcrRequired { .. }))
        );
    }
}

/// 凸clipの外接矩形内でも、三角形の外側の画素は透明にする。
#[test]
fn triangle_clip_masks_pixels_instead_of_only_cropping_bounds() {
    let parsed = parse_pdf_bytes(
        "owned-triangle-image.pdf",
        &fixture(
            "/Fm Do",
            "0 0 m 10 0 l 0 10 l h W n 10 0 0 10 0 0 cm /Im Do",
            "",
        ),
    )
    .unwrap();
    let image = &parsed.image_elements[0];
    let pixels = decode_filter0_png_to_rgba(&DecodeBudget::default(), &image.png_bytes).unwrap();
    let mut transparent = 0;
    let mut colored = 0;
    for (index, pixel) in pixels.rgba.chunks_exact(4).enumerate() {
        let x = index % pixels.width as usize;
        let y = index / pixels.width as usize;
        let distance = (x as f64 + 0.5) / f64::from(pixels.width) + 1.
            - (y as f64 + 0.5) / f64::from(pixels.height);
        if (distance - 1.).abs() < 1e-5 {
            continue;
        }
        if distance < 1. {
            assert_eq!(pixel, [255, 0, 0, 255]);
            colored += 1;
        } else {
            assert_eq!(pixel, [0, 0, 0, 0]);
            transparent += 1;
        }
    }
    assert!(transparent > 0 && colored > 0);
}

/// 切り抜き画像の拡大画素数・PNG保存量を、元画像の一画素だけで判定しない。
#[test]
fn clipped_images_keep_resource_limit_errors() {
    let pdf = fixture("/Fm Do", "2 0 6 10 re W n 20 0 0 20 -5 -5 cm /Im Do", "");
    for (limits, resource) in [
        (
            ParseLimits {
                max_image_pixels: 1,
                ..Default::default()
            },
            "image pixels",
        ),
        (
            ParseLimits {
                max_total_image_storage_bytes: 0,
                ..Default::default()
            },
            "image storage bytes",
        ),
    ] {
        assert!(
            matches!(parse_pdf_bytes_with_limits("owned-limited-form.pdf", &pdf, &limits), Err(ParseError::ResourceLimit { resource: actual, .. }) if actual == resource)
        );
    }
}

/// グループの分離・knockout・未知型を通常の画像描画と混同しない。
#[test]
fn unsupported_group_attributes_keep_image_diagnostics() {
    for group in [
        "/S /Transparency /I true",
        "/S /Transparency /K true",
        "/S /Unknown",
        "/S /Transparency /CS /DeviceCMYK",
    ] {
        let parsed = parse_pdf_bytes(
            "owned-unsupported-group.pdf",
            &fixture("/Fm Do", "/Im Do", &format!("/Group << {group} >>")),
        )
        .unwrap();
        assert_eq!(parsed.image_elements.len(), 1);
        assert!(parsed.quality.issues.iter().any(|issue| matches!(
            issue,
            QualityIssue::ImageReplaced {
                object_number: Some(6),
                ..
            }
        )));
    }
}

/// 不透明・非分離のRGBグループでは元のRGB画素を保持するが、Form診断は残す。
#[test]
fn opaque_rgb_group_preserves_original_image_pixels() {
    let parsed = parse_pdf_bytes(
        "owned-rgb-group.pdf",
        &fixture(
            "/Fm Do",
            "/Im Do",
            "/Group << /Type /Group /S /Transparency /CS /DeviceRGB /I false /K false >>",
        ),
    )
    .unwrap();
    let pixels = decode_filter0_png_to_rgba(
        &DecodeBudget::default(),
        &parsed.image_elements[0].png_bytes,
    )
    .unwrap();
    assert_eq!(pixels.rgba, [255, 0, 0, 255]);
    assert!(
        !parsed
            .quality
            .issues
            .iter()
            .any(|issue| matches!(issue, QualityIssue::ImageReplaced { .. }))
    );
    assert!(parsed.quality.issues.iter().any(|issue| matches!(
        issue,
        QualityIssue::UnsupportedXObject {
            reason: crate::UnsupportedXObjectReason::Form,
            ..
        }
    )));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Form BBox・本文clip・画像範囲の解析的な区間交差と出力座標を照合する。
    #[test]
    fn cropped_image_bounds_match_independent_intervals(
        x in -10i16..15, y in -10i16..15, width in 1u16..20, height in 1u16..20,
        cx in -5i16..10, cy in -5i16..10, cw in 1u16..15, ch in 1u16..15,
    ) {
        let body = format!("{cx} {cy} {cw} {ch} re W n {width} 0 0 {height} {x} {y} cm /Im Do");
        let pdf = fixture("/Fm Do", &body, "");
        let parsed = parse_pdf_bytes("owned-generated-crop.pdf", &pdf).unwrap();
        let left = 0.max(i32::from(cx)).max(i32::from(x));
        let bottom = 0.max(i32::from(cy)).max(i32::from(y));
        let right = 10.min(i32::from(cx) + i32::from(cw)).min(i32::from(x) + i32::from(width));
        let top = 10.min(i32::from(cy) + i32::from(ch)).min(i32::from(y) + i32::from(height));
        if left >= right || bottom >= top { prop_assert!(parsed.image_elements.is_empty()); }
        else {
            prop_assert_eq!(parsed.image_elements.len(), 1);
            prop_assert_eq!(parsed.image_elements[0].bounds, ParsedBounds { x0: 2. * left as f32 + 4., y0: 3. * bottom as f32 + 5., x1: 2. * right as f32 + 4., y1: 3. * top as f32 + 5. });
            let pixels = decode_filter0_png_to_rgba(&DecodeBudget::default(), &parsed.image_elements[0].png_bytes).unwrap();
            prop_assert!(pixels.rgba.chunks_exact(4).all(|pixel| pixel == [255, 0, 0, 255]));
        }
    }
}
