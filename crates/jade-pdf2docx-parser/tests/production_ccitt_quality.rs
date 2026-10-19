//! CCITTの不足行を成功扱いで白補完しない公開API契約。
#[path = "support/ccitt_pdf.rs"]
mod fixture;
use fixture::{ccitt_pdf, encoded_rows};
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Read;

/// filter 0のPNG行を展開して、符号化済みの白黒画素を比較する。
fn pixels(png: &[u8]) -> Vec<u8> {
    let mut cursor = 8;
    let mut idat = Vec::new();
    while cursor + 12 <= png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
        }
        cursor += length + 12;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    raw
}

/// 不足行のある画像を診断し、strictでは同じreportで拒否する。
fn assert_missing_rows(group_four: bool, encoded: usize, declared: usize) {
    let pdf = ccitt_pdf(group_four, declared, &encoded_rows(group_four, encoded));
    let parsed =
        parse_pdf_bytes_with_options("short-ccitt.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [
            QualityIssue::ImageReplaced {
                object_number: Some(4),
                reason: ImageReplacementReason::DecodeFailed,
            },
            QualityIssue::OcrRequired { page: 0 }
        ]
    );
    assert_eq!(parsed.image_elements.len(), 1);
    assert!(parsed.text_preview.contains("retained text"));
    match parse_pdf_bytes_with_options(
        "short-ccitt.pdf",
        &pdf,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    ) {
        Err(ParseError::QualityRejected { report, .. }) => assert_eq!(report, parsed.quality),
        other => panic!("expected CCITT quality rejection: {other:?}"),
    }
}

/// Group 3で、終了markerより後の存在しない行を白で埋めない。
#[test]
fn missing_group_three_rows_are_reported_and_strict_rejects() {
    assert_missing_rows(false, 1, 2);
    assert_missing_rows(false, 2, 5);
}

/// Group 4依存decoderの白補完を成功扱いにしない。
#[test]
fn missing_group_four_rows_are_reported_and_strict_rejects() {
    assert_missing_rows(true, 1, 2);
    assert_missing_rows(true, 2, 5);
}

/// Group 3の短い/長い行を、宣言幅への白補完や切り捨てで隠さない。
#[test]
fn group_three_wrong_row_width_is_reported() {
    use fax::{BitWriter, Bits, VecWriter};
    for run in [
        Bits {
            data: 0b000111,
            len: 6,
        },
        Bits {
            data: 0b1011,
            len: 4,
        },
    ] {
        let mut writer = VecWriter::new();
        writer.write(fax::maps::EOL).unwrap();
        writer.write(run).unwrap();
        for _ in 0..6 {
            writer.write(fax::maps::EOL).unwrap();
        }
        let pdf = ccitt_pdf(false, 1, &writer.finish());
        let parsed =
            parse_pdf_bytes_with_options("wrong-width.pdf", &pdf, &ParseOptions::default())
                .unwrap();
        assert_eq!(
            parsed.quality.issues,
            [
                QualityIssue::ImageReplaced {
                    object_number: Some(4),
                    reason: ImageReplacementReason::DecodeFailed,
                },
                QualityIssue::OcrRequired { page: 0 }
            ]
        );
        assert!(matches!(
            parse_pdf_bytes_with_options(
                "wrong-width.pdf",
                &pdf,
                &ParseOptions {
                    strict: true,
                    ..Default::default()
                }
            ),
            Err(ParseError::QualityRejected { .. })
        ));
    }
}

/// 未対応の正のKをGroup 4として復号し、異なる符号化方式を成功扱いにしない。
#[test]
fn mixed_ccitt_mode_does_not_accept_group_four_data() {
    let mut pdf = ccitt_pdf(false, 2, &encoded_rows(true, 2));
    let k = pdf.windows(4).position(|bytes| bytes == b"/K 0").unwrap();
    pdf[k + 3] = b'1';
    let parsed =
        parse_pdf_bytes_with_options("mixed-ccitt.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [
            QualityIssue::ImageReplaced {
                object_number: Some(4),
                reason: ImageReplacementReason::DecodeFailed,
            },
            QualityIssue::OcrRequired { page: 0 }
        ]
    );
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "mixed-ccitt.pdf",
            &pdf,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}

/// 完全な白黒行は、明示的な全白行も含めて保持し、本文確認要求だけを残す。
#[test]
fn complete_ccitt_rows_preserve_pixels_and_require_text_verification() {
    for group_four in [false, true] {
        for rows in 1..=4 {
            let pdf = ccitt_pdf(group_four, rows, &encoded_rows(group_four, rows));
            let parsed = parse_pdf_bytes_with_options(
                "complete-ccitt.pdf",
                &pdf,
                &ParseOptions {
                    strict: false,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                parsed.quality.issues,
                [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
            );
            assert_eq!(parsed.image_elements.len(), 1);
            let image = &parsed.image_elements[0].png_bytes;
            assert_eq!(u32::from_be_bytes(image[16..20].try_into().unwrap()), 3);
            assert_eq!(
                u32::from_be_bytes(image[20..24].try_into().unwrap()),
                rows as u32
            );
            let raw = pixels(image);
            assert_eq!(raw.len(), rows * 10);
            for row in 0..rows {
                let gray = if row % 2 == 0 { 0 } else { 255 };
                assert_eq!(raw[row * 10], 0);
                assert_eq!(raw[row * 10 + 1..row * 10 + 10], [gray; 9]);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意の不足行数で、Group 3/4とも診断とstrict拒否を保持する。
    #[test]
    fn missing_row_quality_matches_declared_height(group_four in any::<bool>(), encoded in 1usize..=8, missing in 1usize..=8) {
        assert_missing_rows(group_four, encoded, encoded + missing);
    }
}
