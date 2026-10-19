//! CCITTのfilter極性とsource Decodeを独立した画素期待値で固定する。
#[path = "support/ccitt_mask.rs"]
mod codec;
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// 二値sampleを持つ画像の公開PNGを、正式xrefを使うparserで取得する。
fn pixels(black: [bool; 2], black_is_one: bool, decode: &str, mask: bool) -> Vec<u8> {
    let attributes = format!(
        "/ColorSpace /DeviceGray /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 2 /BlackIs1 {black_is_one} >> /Decode {decode} {}",
        if mask { "/SMask 5 0 R" } else { "" }
    );
    let pdf = fixture::source_image_pdf_with_bits(
        &attributes,
        &codec::encoded_mask(&[black.to_vec()]),
        "",
        1,
    );
    let parsed =
        parse_pdf_bytes_with_options("ccitt-source.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert!(parsed.text_preview.contains("retained text"));
    assert_eq!(parsed.image_elements.len(), 1);
    fixture::scanline(&parsed.image_elements[0].png_bytes)
}

/// BlackIs1とDecode反転は別々に適用し、二重反転は元の色へ戻す。
#[test]
fn source_polarity_and_inverse_decode_are_independent() {
    for black_is_one in [false, true] {
        for decode in ["[1 0]", "8 0 R", "[9 0 R 10 0 R]"] {
            let first = if black_is_one { 0 } else { 255 };
            assert_eq!(
                pixels([true, false], black_is_one, decode, false),
                [
                    0,
                    first,
                    first,
                    first,
                    255 - first,
                    255 - first,
                    255 - first
                ]
            );
        }
    }
}

/// 色のsource Decodeを透明度へ適用せず、SMaskのsampleをそのまま保持する。
#[test]
fn inverse_source_decode_preserves_soft_mask_alpha() {
    assert_eq!(
        pixels([true, false], false, "[1 0]", true),
        [0, 255, 255, 255, 85, 0, 0, 0, 255]
    );
}

proptest! {
    /// 1 bitの端点写像を、正規化・clip・丸めだけの独立モデルで検査する。
    #[test]
    fn source_endpoints_follow_gray_mapping(first in any::<bool>(), second in any::<bool>(), black_is_one in any::<bool>(), low in -4i16..=8, high in -4i16..=8) {
        let decode = format!("[{} {}]", f64::from(low)/4.0, f64::from(high)/4.0);
        let mut expected = vec![0];
        for black in [first, second] {
            let sample_is_one = black == black_is_one;
            let endpoint = if sample_is_one { high } else { low };
            let gray = (f64::from(endpoint)/4.0).clamp(0.0, 1.0).mul_add(255.0, 0.0).round() as u8;
            expected.extend([gray;3]);
        }
        prop_assert_eq!(pixels([first,second], black_is_one, &decode, false), expected);
    }
}

/// 独立したGroup 3固定符号でも、Grayの有限端点を復号後に写像する。
#[test]
fn group_three_source_decode_maps_endpoints() {
    let pdf = fixture::source_image_pdf_with_bits(
        "/ColorSpace /DeviceGray /Filter /CCITTFaxDecode /DecodeParms << /K 0 /Columns 2 /Rows 1 /EndOfBlock false >> /Decode [.25 .75]",
        &[0x35, 0x43, 0x80],
        "",
        1,
    );
    let parsed =
        parse_pdf_bytes_with_options("group-three.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert_eq!(
        fixture::scanline(&parsed.image_elements[0].png_bytes),
        [0, 64, 64, 64, 191, 191, 191]
    );
}

/// 不正Decodeと未対応のsource色/bit数/stencilは、画像置換診断とstrict拒否を維持する。
#[test]
fn unsupported_or_invalid_source_decode_is_rejected() {
    use jade_pdf2docx_parser::{ImageReplacementReason, ParseError};
    for (color, bits, extra, decode) in [
        ("/DeviceGray", 1, "", "[]"),
        ("/DeviceGray", 1, "", "[1 0 1 0]"),
        ("/DeviceGray", 1, "", "[NaN 0]"),
        ("/DeviceGray", 1, "", "[1e1 0]"),
        ("/DeviceGray", 1, "", "12 0 R"),
        ("/DeviceRGB", 1, "", "[1 0 1 0 1 0]"),
        ("/DeviceGray", 8, "", "[1 0]"),
        ("/DeviceGray", 1, "/ImageMask true", "[1 0]"),
    ] {
        let attributes = format!(
            "/ColorSpace {color} /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 2 >> /Decode {decode} {extra}"
        );
        let pdf = fixture::source_image_pdf_with_bits(
            &attributes,
            &codec::encoded_mask(&[vec![true, false]]),
            "",
            bits,
        );
        let parsed =
            parse_pdf_bytes_with_options("invalid-source.pdf", &pdf, &ParseOptions::default())
                .unwrap();
        assert!(
            parsed
                .quality
                .issues
                .contains(&QualityIssue::ImageReplaced {
                    object_number: Some(4),
                    reason: ImageReplacementReason::DecodeFailed
                }),
            "{attributes}"
        );
        assert!(
            matches!(
                parse_pdf_bytes_with_options(
                    "invalid-source.pdf",
                    &pdf,
                    &ParseOptions {
                        strict: true,
                        ..Default::default()
                    }
                ),
                Err(ParseError::QualityRejected { .. })
            ),
            "{attributes}"
        );
    }
}
