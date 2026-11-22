//! 単一要素のDevice色空間を、原本sampleから白い代替画像へ変えない。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// 正式xrefの所有PDFを解析し、画像診断とPNGを取り出す。
fn parsed(color: &str, samples: &[u8]) -> jade_pdf2docx_parser::ParsedDocument {
    let source = fixture::source_image_pdf(
        &format!("/ColorSpace {color} /Filter /FlateDecode"),
        &fixture::compressed(samples),
        "",
    );
    parse_pdf_bytes_with_options("owned-color.pdf", &source, &ParseOptions::default()).unwrap()
}

/// 名前形式の既存対照は、そのまま実際のsampleとOCR診断を保つ。
#[test]
fn device_name_control_preserves_source_pixels() {
    let p = parsed("/DeviceRGB", &[12, 34, 56, 78, 90, 123]);
    assert_eq!(p.quality.issues, [QualityIssue::OcrRequired { page: 0 }]);
    assert_eq!(
        fixture::scanline(&p.image_elements[0].png_bytes),
        [0, 12, 34, 56, 78, 90, 123]
    );
}

/// RGBとGrayの一要素配列は、空白・コメント・name escapeがあっても画素を保つ。
#[test]
fn singleton_device_arrays_preserve_source_pixels() {
    for color in ["[/DeviceRGB]", "[ /Device#52GB %owned\n ]"] {
        let p = parsed(color, &[12, 34, 56, 78, 90, 123]);
        assert_eq!(p.quality.issues, [QualityIssue::OcrRequired { page: 0 }]);
        assert_eq!(
            fixture::scanline(&p.image_elements[0].png_bytes),
            [0, 12, 34, 56, 78, 90, 123]
        );
    }
    let p = parsed("[/DeviceGray]", &[17, 231]);
    assert_eq!(p.quality.issues, [QualityIssue::OcrRequired { page: 0 }]);
    assert_eq!(
        fixture::scanline(&p.image_elements[0].png_bytes),
        [0, 17, 17, 17, 231, 231, 231]
    );
}

/// 配列自体が間接objectでも、元の参照世代を維持してRGBを解決する。
#[test]
fn indirect_singleton_color_array_is_resolved() {
    let source = fixture::image_with_resources(
        "/ColorSpace 12 0 R /Filter /FlateDecode",
        &fixture::compressed(&[12, 34, 56, 78, 90, 123]),
        8,
        b"q 20 0 0 10 0 0 cm /Im Do Q BT /F1 12 Tf 10 5 Td (retained text) Tj ET",
        "<< /XObject << /Im 4 0 R >> /Font << /F1 7 0 R >> >>",
        false,
        &[b"[/DeviceRGB]"],
    );
    let p = parse_pdf_bytes_with_options(
        "owned-indirect-color.pdf",
        &source,
        &ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(p.quality.issues, [QualityIssue::OcrRequired { page: 0 }]);
    assert_eq!(
        fixture::scanline(&p.image_elements[0].png_bytes),
        [0, 12, 34, 56, 78, 90, 123]
    );
}

/// 未知の型や余分な要素をDevice色と推測せず、従来の置換診断を保つ。
#[test]
fn malformed_or_ambiguous_arrays_remain_diagnosed() {
    for color in [
        "[]",
        "[/DeviceRGB null]",
        "[/DeviceRGB /DeviceGray]",
        "[[/DeviceRGB]]",
        "[/Unknown]",
    ] {
        let p = parsed(color, &[12, 34, 56, 78, 90, 123]);
        assert!(
            p.quality
                .issues
                .iter()
                .any(|i| matches!(i, QualityIssue::ImageReplaced { .. })),
            "{color}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意の二画素RGBは、一要素配列で囲んでも同じPNGと診断になる。
    #[test]
    fn singleton_wrapper_does_not_change_rgb_samples(samples in any::<[u8; 6]>()) {
        let plain = parsed("/DeviceRGB", &samples);
        let wrapped = parsed("[/DeviceRGB]", &samples);
        prop_assert_eq!(plain.quality.issues, wrapped.quality.issues);
        prop_assert_eq!(&plain.image_elements[0].png_bytes, &wrapped.image_elements[0].png_bytes);
    }
}
