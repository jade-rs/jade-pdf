//! Group 3のDecodeParmsと符号化行を公開APIで照合する。
#[path = "support/ccitt_pdf.rs"]
mod fixture;
use fax::{BitWriter, Bits, VecWriter};
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Read;

/// 3画素の交互白黒行を、EOL/alignment/RTCを独立に指定して符号化する。
fn rows_data(rows: usize, eol: bool, aligned: bool, rtc: bool) -> Vec<u8> {
    let mut writer = VecWriter::new();
    for row in 0..rows {
        if aligned {
            writer.pad();
        }
        if eol {
            writer.write(fax::maps::EOL).unwrap();
        }
        if row % 2 == 0 {
            writer
                .write(Bits {
                    data: 0b00110101,
                    len: 8,
                })
                .unwrap();
            writer.write(Bits { data: 0b10, len: 2 }).unwrap();
        } else {
            writer
                .write(Bits {
                    data: 0b1000,
                    len: 4,
                })
                .unwrap();
        }
    }
    if rtc {
        for _ in 0..6 {
            if aligned {
                writer.pad();
            }
            writer.write(fax::maps::EOL).unwrap();
        }
    }
    writer.finish()
}

/// PNGのscanlineを復元する。
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

/// 正常な行を抽出し、寸法・本文・画素を比較する。
fn assert_rows(rows: usize, eol: bool, aligned: bool, rtc: bool) {
    let data = rows_data(rows, eol, aligned, rtc);
    let parameters = format!("/K 0 /EndOfLine {eol} /EncodedByteAlign {aligned} /EndOfBlock {rtc}");
    let pdf = fixture::ccitt_pdf_with_parameters(rows, &data, &parameters);
    let parsed = parse_pdf_bytes_with_options(
        "ccitt-parameters.pdf",
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
    assert!(parsed.text_preview.contains("retained text"));
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

/// EOL/RTCのない正常な行を、宣言行数とbyte alignmentで復号する。
#[test]
fn group_three_without_eol_or_rtc_preserves_pixels() {
    for aligned in [false, true] {
        for rows in 1..=6 {
            assert_rows(rows, false, aligned, false);
        }
    }
}

/// EOLなしでもRTCがある正常データを保持する。
#[test]
fn group_three_without_eol_with_rtc_preserves_pixels() {
    for aligned in [false, true] {
        assert_rows(4, false, aligned, true);
    }
}

/// EndOfLineを要求する行は、RTCがなくても行数で復号する。
#[test]
fn group_three_eol_without_rtc_preserves_pixels() {
    for aligned in [false, true] {
        assert_rows(4, true, aligned, false);
    }
}

/// EndOfLineの既定falseでも、存在するEOLを受け入れて画素を保持する。
#[test]
fn optional_eol_is_accepted_with_default_parameters() {
    let pdf = fixture::ccitt_pdf(false, 2, &fixture::encoded_rows(false, 2));
    let parsed = parse_pdf_bytes_with_options(
        "optional-eol.pdf",
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
    assert_eq!(
        pixels(&parsed.image_elements[0].png_bytes),
        [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255
        ]
    );
}

/// byte alignmentの非ゼロpaddingは画素として読み飛ばさず拒否する。
#[test]
fn nonzero_alignment_padding_is_reported() {
    let mut data = rows_data(2, false, true, false);
    data[1] |= 1;
    let pdf = fixture::ccitt_pdf_with_parameters(
        2,
        &data,
        "/K 0 /EncodedByteAlign true /EndOfBlock false",
    );
    let result = parse_pdf_bytes_with_options(
        "bad-padding.pdf",
        &pdf,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    );
    assert!(
        matches!(result, Err(ParseError::QualityRejected { report, .. }) if report.issues == [QualityIssue::ImageReplaced { object_number: Some(4), reason: ImageReplacementReason::DecodeFailed }, QualityIssue::OcrRequired { page: 0 }])
    );
}

/// 必須markerの欠落と不足行を診断し、strictで拒否する。
#[test]
fn missing_required_markers_or_rows_are_reported() {
    for (encoded, declared, data_eol, data_rtc, required_eol, required_rtc) in [
        (2, 2, false, true, true, true),
        (2, 2, true, false, true, true),
        (1, 2, false, false, false, false),
    ] {
        let data = rows_data(encoded, data_eol, false, data_rtc);
        let pdf = fixture::ccitt_pdf_with_parameters(
            declared,
            &data,
            &format!("/K 0 /EndOfLine {required_eol} /EndOfBlock {required_rtc}"),
        );
        let normal =
            parse_pdf_bytes_with_options("bad-markers.pdf", &pdf, &ParseOptions::default())
                .unwrap();
        assert_eq!(
            normal.quality.issues,
            [
                QualityIssue::ImageReplaced {
                    object_number: Some(4),
                    reason: ImageReplacementReason::DecodeFailed
                },
                QualityIssue::OcrRequired { page: 0 }
            ]
        );
        assert!(
            matches!(parse_pdf_bytes_with_options("bad-markers.pdf", &pdf, &ParseOptions { strict: true, ..Default::default() }), Err(ParseError::QualityRejected { report, .. }) if report == normal.quality)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 行数とEOL/alignment/RTCの組み合わせに対して白黒画素を保持する。
    #[test]
    fn group_three_parameters_match_pixel_model(rows in 1usize..=16, eol in any::<bool>(), aligned in any::<bool>(), rtc in any::<bool>()) {
        assert_rows(rows, eol, aligned, rtc);
    }
}
