//! source Decodeの公開画素と、処理できない属性の品質拒否を固定する。
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};

/// RGB/Gray/IndexedのDecode済みPNGと本文を保持し、画像本文の確認要求だけを残す。
#[test]
fn decode_preserves_pixels_and_text() {
    for (attributes, samples, mask, expected) in [
        (
            "/ColorSpace /DeviceGray /Filter /FlateDecode /Decode 8 0 R",
            vec![0, 85],
            "",
            vec![0, 255, 255, 255, 170, 170, 170],
        ),
        (
            "/ColorSpace /DeviceRGB /Filter /FlateDecode /Decode [1 0 1 0 1 0]",
            vec![10, 20, 30, 40, 50, 60],
            "",
            vec![0, 245, 235, 225, 215, 205, 195],
        ),
        (
            "/ColorSpace [/Indexed /DeviceRGB 1 <ff00000000ff>] /Filter /FlateDecode /Decode [1 0]",
            vec![0, 255],
            "",
            vec![0, 0, 0, 255, 255, 0, 0],
        ),
        (
            "/ColorSpace /DeviceGray /Filter /FlateDecode /Decode [.1 .5] /SMask 5 0 R",
            vec![128, 64],
            "/Matte [.2]",
            vec![0, 128, 128, 128, 85, 51, 51, 51, 255],
        ),
    ] {
        let pdf = fixture::source_image_pdf(attributes, &fixture::compressed(&samples), mask);
        let parsed = parse_pdf_bytes_with_options(
            "decode.pdf",
            &pdf,
            &ParseOptions {
                strict: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            parsed.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        assert!(parsed.text_preview.contains("retained text"));
        assert_eq!(parsed.image_elements.len(), 1);
        assert_eq!(
            fixture::scanline(&parsed.image_elements[0].png_bytes),
            expected,
            "{attributes}"
        );
    }
}

/// 不正・未対応Decodeは通常時も診断し、strictの本文成功へ格下げしない。
#[test]
fn malformed_decode_is_reported_and_strictly_rejected() {
    for decode in ["[]", "false", "[1 0 1 0]", "[NaN 0]", "[1e1 0]", "12 0 R"] {
        let attributes = format!("/ColorSpace /DeviceGray /Filter /FlateDecode /Decode {decode}");
        let pdf = fixture::source_image_pdf(&attributes, &fixture::compressed(&[0, 85]), "");
        let parsed =
            parse_pdf_bytes_with_options("decode.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert_eq!(
            parsed.quality.issues,
            [
                QualityIssue::ImageReplaced {
                    object_number: Some(4),
                    reason: ImageReplacementReason::DecodeFailed
                },
                QualityIssue::OcrRequired { page: 0 }
            ]
        );
        assert!(
            matches!(
                parse_pdf_bytes_with_options(
                    "decode.pdf",
                    &pdf,
                    &ParseOptions {
                        strict: true,
                        ..Default::default()
                    }
                ),
                Err(ParseError::QualityRejected { .. })
            ),
            "{decode}"
        );
    }
}
