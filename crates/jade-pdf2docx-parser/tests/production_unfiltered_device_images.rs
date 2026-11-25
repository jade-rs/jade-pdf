//! 無圧縮Device画像の画素と、未対応属性・明示予算の拒否を公開APIで検証する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{
    ParseError, ParseLimits, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

/// 正式xrefの所有画像を、PDFiumに依存しない公開bytes APIで読む。
fn parse(attributes: &str, samples: &[u8]) -> jade_pdf2docx_parser::ParsedDocument {
    let pdf = fixture::source_image_pdf(attributes, samples, "");
    parse_pdf_bytes_with_options("owned-raw-device.pdf", &pdf, &ParseOptions::default()).unwrap()
}

/// sourceのRGB六成分を白い代替画像へ変えず、OCR未検証の診断は保持する。
#[test]
fn unfiltered_rgb_preserves_source_samples() {
    let parsed = parse("/ColorSpace /DeviceRGB", &[16, 32, 48, 250, 128, 0]);
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }],
        "valid unfiltered device samples must decode"
    );
    assert_eq!(
        fixture::scanline(&parsed.image_elements[0].png_bytes),
        [0, 16, 32, 48, 250, 128, 0]
    );
    assert!(parsed.text_preview.contains("retained text"));
}

/// Grayの明示DecodeをRGB展開の前に適用する。
#[test]
fn unfiltered_gray_applies_source_decode() {
    let parsed = parse("/ColorSpace /DeviceGray /Decode [1 0]", &[0, 85]);
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }],
        "valid unfiltered device samples must decode"
    );
    assert_eq!(
        fixture::scanline(&parsed.image_elements[0].png_bytes),
        [0, 255, 255, 255, 170, 170, 170]
    );
}

/// 未対応色空間・sample長・filter・maskを推測で補完せず、strictも失敗させる。
#[test]
fn unsupported_or_malformed_raw_attributes_remain_rejected() {
    for (attrs, data) in [
        ("/ColorSpace /DeviceRGB", &[1, 2, 3, 4, 5][..]),
        ("/ColorSpace /DeviceRGB", &[1, 2, 3, 4, 5, 6, 7][..]),
        (
            "/ColorSpace /DeviceRGB /Filter /JPXDecode",
            &[1, 2, 3, 4, 5, 6][..],
        ),
        ("/ColorSpace /DeviceRGB /Filter []", &[1, 2, 3, 4, 5, 6][..]),
        (
            "/ColorSpace /DeviceRGB /Decode [0 1]",
            &[1, 2, 3, 4, 5, 6][..],
        ),
        (
            "/ColorSpace /DeviceRGB /DecodeParms << /Predictor 12 >>",
            &[1, 2, 3, 4, 5, 6][..],
        ),
        (
            "/ColorSpace /DeviceRGB /Mask [0 0 0 0 0 0]",
            &[1, 2, 3, 4, 5, 6][..],
        ),
        (
            "/ColorSpace /DeviceRGB /SMask 5 0 R",
            &[1, 2, 3, 4, 5, 6][..],
        ),
        ("/ColorSpace /DeviceCMYK", &[1, 2, 3, 4, 5, 6, 7, 8][..]),
    ] {
        let pdf = fixture::source_image_pdf(attrs, data, "");
        let parsed =
            parse_pdf_bytes_with_options("invalid-raw.pdf", &pdf, &ParseOptions::default())
                .unwrap();
        assert!(
            parsed
                .quality
                .issues
                .iter()
                .any(|i| matches!(i, QualityIssue::ImageReplaced { .. })),
            "{attrs}"
        );
        assert!(parsed.text_preview.contains("retained text"));
        assert!(
            parse_pdf_bytes_with_options(
                "invalid-raw.pdf",
                &pdf,
                &ParseOptions {
                    strict: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}

/// 画素の上限ちょうどを許し、一画素不足をplaceholder成功へ置き換えない。
#[test]
fn raw_image_pixel_budget_is_inclusive() {
    let pdf = fixture::source_image_pdf("/ColorSpace /DeviceRGB", &[1, 2, 3, 4, 5, 6], "");
    let mut options = ParseOptions {
        limits: ParseLimits {
            max_image_pixels: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let parsed = parse_pdf_bytes_with_options("raw-limit.pdf", &pdf, &options).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }],
        "valid unfiltered device samples must decode"
    );
    options.limits.max_image_pixels = 1;
    assert!(matches!(
        parse_pdf_bytes_with_options("raw-limit.pdf", &pdf, &options),
        Err(ParseError::ResourceLimit {
            resource: "image pixels",
            ..
        })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig { cases:24, failure_persistence:Some(Box::new(FileFailurePersistence::Direct("tests/production_unfiltered_device_images.proptest-regressions"))), ..ProptestConfig::default() })]
    /// 二画素の全成分を、圧縮codecや代替画像を介さず可逆に保持する。
    #[test]
    fn arbitrary_rgb_samples_are_lossless(samples in any::<[u8;6]>()) {
        let parsed=parse("/ColorSpace /DeviceRGB", &samples);
        prop_assert_eq!(&parsed.quality.issues,&[QualityIssue::OcrRequired{page:0}],"valid unfiltered device samples must decode");
        let actual=fixture::scanline(&parsed.image_elements[0].png_bytes);
        prop_assert_eq!(&actual[1..],samples);
    }
}
