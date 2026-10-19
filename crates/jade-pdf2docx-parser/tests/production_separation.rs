//! Separationのsampled tintとICC alternateを独立画素へ照合する。
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;

#[path = "support/separation.rs"]
mod separation;
use separation::pdf as sampled_pdf;

/// source DecodeとMatteをtint補間より先に適用し、最後だけ量子化する。
#[test]
fn sampled_tint_applies_source_decode_and_matte_before_interpolation() {
    let function = separation::LINEAR_RGB;
    for (attributes, samples, expected) in [
        ("", [64, 255], vec![0, 64, 128, 191, 255, 128, 0]),
        (
            "/SMask 13 0 R",
            [64, 255],
            vec![0, 90, 128, 165, 85, 255, 128, 0, 255],
        ),
        (
            "/SMask 13 0 R /Decode [.1 .3]",
            [64, 255],
            vec![0, 13, 128, 242, 85, 77, 128, 179, 255],
        ),
        (
            "/Decode [1 0]",
            [64, 255],
            vec![0, 191, 128, 64, 0, 128, 255],
        ),
        (
            "/SMask 5 0 R",
            [64, 255],
            vec![0, 64, 128, 191, 85, 255, 128, 0, 255],
        ),
    ] {
        let pdf = sampled_pdf(function, attributes, &samples);
        let parsed =
            parse_pdf_bytes_with_options("sampled.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert!(
            !parsed
                .quality
                .issues
                .iter()
                .any(|i| matches!(i, QualityIssue::ImageReplaced { .. })),
            "{attributes}"
        );
        assert_eq!(
            fixture::scanline(&parsed.image_elements[0].png_bytes),
            expected
        );
    }
}

/// 未対応のcubic補間・不正配列・過大tableを推測せず診断する。
#[test]
fn invalid_sampled_functions_are_reported_and_rejected_in_strict_mode() {
    for function in [
        "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [2] /BitsPerSample 8 /Order 3",
        "/FunctionType 0 /Domain [0 0] /Range [0 1 0 1 0 1] /Size [2] /BitsPerSample 8",
        "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [4097] /BitsPerSample 8",
        "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [2] /BitsPerSample 8 /Encode [0]",
        "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [2] /BitsPerSample 8 /Decode [0 1]",
        "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [2] /BitsPerSample 8 /DecodeParms << /Predictor 2 >>",
    ] {
        let pdf = sampled_pdf(function, "", &[64, 255]);
        let parsed =
            parse_pdf_bytes_with_options("sampled.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert!(
            parsed
                .quality
                .issues
                .iter()
                .any(|i| matches!(i, QualityIssue::ImageReplaced { .. })),
            "{function}"
        );
        let options = ParseOptions {
            strict: true,
            ..Default::default()
        };
        assert!(parse_pdf_bytes_with_options("sampled.pdf", &pdf, &options).is_err());
    }
}

/// 有限sampleの端点保持と反転は整数oracleへ照合する。
#[test]
fn sampled_tint_sample_and_inverse_properties() {
    use proptest::prelude::*;
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 48,
        ..Default::default()
    });
    runner
        .run(&(any::<u8>(), any::<bool>()), |(sample, inverse)| {
            let attributes = if inverse { "/Decode [1 0]" } else { "" };
            let pdf = sampled_pdf(separation::LINEAR_RGB, attributes, &[sample, 255]);
            let parsed =
                parse_pdf_bytes_with_options("property.pdf", &pdf, &ParseOptions::default())
                    .unwrap();
            let value = if inverse { 255 - sample } else { sample };
            let last = if inverse { 0 } else { 255 };
            prop_assert_eq!(
                fixture::scanline(&parsed.image_elements[0].png_bytes),
                vec![0, value, 128, 255 - value, last, 128, 255 - last]
            );
            Ok(())
        })
        .unwrap();
}
