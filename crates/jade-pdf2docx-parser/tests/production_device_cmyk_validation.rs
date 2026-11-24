//! CMYK JPEGの構文不変条件と、不正metadata・資源上限による拒否を検証する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
#[path = "support/owned_device_cmyk_jpeg.rs"]
mod owned;
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseLimits, ParseOptions, QualityIssue,
    parse_pdf_bytes_with_options,
};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 名前escape・singleton・間接参照・空白が変わっても同じsource画素を保持する。
    #[test]
    fn equivalent_device_metadata_preserves_encoded_pixels(
        ycck in any::<bool>(), escaped in any::<bool>(), singleton in any::<bool>(),
        indirect in any::<bool>(), identity in any::<bool>(), newline in any::<bool>(),
    ) {
        let jpeg = if ycck { owned::YCCK } else { owned::CMYK };
        let reference = fixture::source_image_pdf("/ColorSpace /DeviceCMYK /Filter /DCTDecode", jpeg, "");
        let reference = parse_pdf_bytes_with_options("reference.pdf", &reference, &ParseOptions::default()).unwrap();
        let name = if escaped { "/DeviceCM#59K" } else { "/DeviceCMYK" };
        let color = if singleton { format!("[{name}]") } else { name.to_owned() };
        let value = if indirect { "12 0 R" } else { &color };
        let decode = if identity { "/Decode 13 0 R" } else { "" };
        let space = if newline { "\n% owned syntax variation\n" } else { " " };
        let attributes = format!("/ColorSpace{space}{value} /Filter /DCTDecode {decode}");
        let pdf = fixture::image_with_resources(
            &attributes, jpeg, 8,
            b"q 20 0 0 10 0 0 cm /Im Do Q BT /F1 12 Tf 10 5 Td (retained text) Tj ET",
            "<< /XObject << /Im 4 0 R >> /Font << /F1 7 0 R >> >>", false,
            &[color.as_bytes(), b"[10 0 R 9 0 R 0 1 0 1 0 1]"],
        );
        let actual = parse_pdf_bytes_with_options("equivalent.pdf", &pdf, &ParseOptions::default()).unwrap();
        prop_assert_eq!(&actual.quality, &reference.quality);
        prop_assert_eq!(&actual.text_preview, &reference.text_preview);
        prop_assert_eq!(&actual.image_elements[0].png_bytes, &reference.image_elements[0].png_bytes);
    }
}

/// Cだけの反転をRGB化より前に適用し、他の三成分をそのまま保持する。
#[test]
fn source_decode_can_invert_only_cyan() {
    for jpeg in [owned::CMYK, owned::YCCK] {
        let pdf = fixture::source_image_pdf(
            "/ColorSpace /DeviceCMYK /Filter /DCTDecode /Decode [1 0 0 1 0 1 0 1]",
            jpeg,
            "",
        );
        let parsed =
            parse_pdf_bytes_with_options("cyan.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert_eq!(
            parsed.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        let rgb = fixture::scanline(&parsed.image_elements[0].png_bytes);
        assert!(rgb[1] < 20 && (80..=180).contains(&rgb[2]) && rgb[3] < 20);
        assert!(rgb[4] > 230 && (60..=160).contains(&rgb[5]) && rgb[6] > 230);
    }
}

/// 復号できない画像でも本文を保持し、strictでは品質欠落を拒否する。
fn assert_replaced(pdf: &[u8]) {
    let parsed =
        parse_pdf_bytes_with_options("invalid.pdf", pdf, &ParseOptions::default()).unwrap();
    assert!(parsed.text_preview.contains("retained text"));
    assert!(parsed.quality.issues.iter().any(|issue| matches!(
        issue,
        QualityIssue::ImageReplaced {
            reason: ImageReplacementReason::DecodeFailed,
            ..
        }
    )));
    assert!(
        parse_pdf_bytes_with_options(
            "invalid.pdf",
            pdf,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        )
        .is_err()
    );
}

/// 成分数と不正Decode・Matteを推測で補完しない。
#[test]
fn malformed_source_metadata_is_reported_and_strictly_rejected() {
    for decode in [
        "[]",
        "[0 1]",
        "[0 1 0 1 0 1]",
        "[0 1 0 1 0 1 0 1 0 1]",
        "[0 1 0 1 0 1 /bad 1]",
        "true",
    ] {
        assert_replaced(&fixture::source_image_pdf(
            &format!("/ColorSpace /DeviceCMYK /Filter /DCTDecode /Decode {decode}"),
            owned::CMYK,
            "",
        ));
    }
    for matte in ["[0 0 0]", "[0 0 0 0 0]", "[0 0 0 /bad]"] {
        assert_replaced(&fixture::source_image_pdf(
            "/ColorSpace /DeviceCMYK /Filter /DCTDecode /SMask 5 0 R",
            owned::CMYK,
            &format!("/Matte {matte}"),
        ));
    }
    assert_replaced(&fixture::source_image_pdf_with_bits(
        "/ColorSpace /DeviceCMYK /Filter /DCTDecode",
        owned::CMYK,
        "",
        4,
    ));
    assert_replaced(&fixture::source_image_pdf_with_dimensions(
        "/ColorSpace /DeviceCMYK /Filter /DCTDecode",
        owned::CMYK,
        "",
        8,
        b"q 20 0 0 10 0 0 cm /Im Do Q BT /F1 12 Tf 10 5 Td (retained text) Tj ET",
        3,
        1,
    ));
}

/// 明示pixel上限の両側を検査し、strictの有無で上限を迂回しない。
#[test]
fn explicit_image_budget_is_inclusive_for_four_component_jpeg() {
    let pdf = fixture::source_image_pdf(
        "/ColorSpace /DeviceCMYK /Filter /DCTDecode",
        owned::YCCK,
        "",
    );
    let mut options = ParseOptions {
        limits: ParseLimits {
            max_image_pixels: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let accepted = parse_pdf_bytes_with_options("limit.pdf", &pdf, &options).unwrap();
    assert_eq!(
        accepted.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    options.limits.max_image_pixels = 1;
    for strict in [false, true] {
        options.strict = strict;
        assert!(matches!(
            parse_pdf_bytes_with_options("limit.pdf", &pdf, &options),
            Err(ParseError::ResourceLimit {
                resource: "image pixels",
                ..
            })
        ));
    }
}
