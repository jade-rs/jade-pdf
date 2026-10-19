//! Group 4の終端・行境界・不正な行幅を公開APIで検証する。
#[path = "support/ccitt_pdf.rs"]
mod fixture;
use fax::{BitWriter, Bits, VecWriter};
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Read;

/// 全白の3画素行をV(0)で符号化し、行境界とEOFBを独立に指定する。
fn white_rows(rows: usize, aligned: bool, eol: bool, terminal_halves: usize) -> Vec<u8> {
    let mut writer = VecWriter::new();
    for _ in 0..rows {
        if aligned {
            writer.pad();
        }
        if eol {
            writer.write(fax::maps::EOL).unwrap();
        }
        writer.write(Bits { data: 1, len: 1 }).unwrap();
    }
    if aligned {
        writer.pad();
    }
    for _ in 0..terminal_halves {
        writer.write(fax::maps::EOL).unwrap();
    }
    writer.finish()
}

/// 異常入力の診断とstrict拒否が一致し、本文を保持することを確認する。
fn assert_rejected(rows: usize, data: &[u8], parameters: &str) {
    let pdf = fixture::ccitt_pdf_with_parameters(rows, data, parameters);
    let parsed =
        parse_pdf_bytes_with_options("group-four.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert!(parsed.text_preview.contains("retained text"));
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
    assert!(
        matches!(parse_pdf_bytes_with_options("group-four.pdf", &pdf, &ParseOptions {
        strict: true, ..Default::default()
    }), Err(ParseError::QualityRejected { report, .. }) if report == parsed.quality)
    );
}

/// PNGのscanlineを復元する。
fn pixels(png: &[u8]) -> Vec<u8> {
    let mut idat = Vec::new();
    let mut cursor = 8;
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

/// 正常入力のPNGを復元し、全行が白であることを確認する。
fn assert_white(rows: usize, data: &[u8], parameters: &str) {
    let pdf = fixture::ccitt_pdf_with_parameters(rows, data, parameters);
    let parsed = parse_pdf_bytes_with_options(
        "group-four.pdf",
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
    let png = &parsed.image_elements[0].png_bytes;
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 3);
    assert_eq!(
        u32::from_be_bytes(png[20..24].try_into().unwrap()),
        rows as u32
    );
    let raw = pixels(png);
    let expected: Vec<u8> = (0..rows)
        .flat_map(|_| std::iter::once(0).chain([255; 9]))
        .collect();
    assert_eq!(raw, expected);
}

/// 既定のEndOfBlockでは24 bitのEOFB全体を必須にする。
#[test]
fn group_four_requires_both_eofb_halves() {
    for halves in [0, 1] {
        assert_rejected(2, &white_rows(2, false, false, halves), "/K -1");
    }
    assert_white(2, &white_rows(2, false, false, 2), "/K -1");
}

/// EndOfBlock falseならEOFBなしの明示的な行数を復号する。
#[test]
fn group_four_without_eofb_preserves_declared_rows() {
    assert_white(
        4,
        &white_rows(4, false, false, 0),
        "/K -1 /EndOfBlock false",
    );
}

/// byte境界のゼロpaddingと必須または任意のEOLを復号する。
#[test]
fn group_four_row_parameters_preserve_pixels() {
    for aligned in [false, true] {
        for eol in [false, true] {
            for end_of_block in [false, true] {
                for required_eol in [false, eol] {
                    let parameters = format!(
                        "/K -1 /EncodedByteAlign {aligned} /EndOfLine {required_eol} /EndOfBlock {end_of_block}"
                    );
                    assert_white(
                        4,
                        &white_rows(4, aligned, eol, usize::from(end_of_block) * 2),
                        &parameters,
                    );
                }
            }
        }
    }
}

/// byte alignmentは行の開始に適用し、EOFB直前のpaddingを必須にしない。
#[test]
fn group_four_aligned_rows_accept_unpadded_terminal() {
    let mut writer = VecWriter::new();
    for _ in 0..4 {
        writer.pad();
        writer.write(Bits { data: 1, len: 1 }).unwrap();
    }
    writer.write(fax::maps::EOL).unwrap();
    writer.write(fax::maps::EOL).unwrap();
    assert_white(4, &writer.finish(), "/K -1 /EncodedByteAlign true");
}

/// 非ゼロpaddingと必須EOLの欠落を成功扱いにしない。
#[test]
fn group_four_invalid_row_boundaries_are_reported() {
    let mut data = white_rows(2, true, false, 0);
    data[0] |= 1;
    assert_rejected(2, &data, "/K -1 /EncodedByteAlign true /EndOfBlock false");
    assert_rejected(2, &white_rows(2, false, false, 2), "/K -1 /EndOfLine true");
}

/// Horizontal/Verticalの過大幅を切り詰めず、不完全な行を拒否する。
#[test]
fn group_four_invalid_width_is_reported() {
    // Horizontal + white(4) + black(0): width 3を超過する。
    let mut horizontal = VecWriter::new();
    horizontal
        .write(Bits {
            data: 0b001,
            len: 3,
        })
        .unwrap();
    horizontal
        .write(fax::maps::white::encode(4).unwrap())
        .unwrap();
    horizontal
        .write(fax::maps::black::encode(0).unwrap())
        .unwrap();
    // Vertical(+1): 初期参照行のb1=3を超過する。
    let mut vertical = VecWriter::new();
    vertical
        .write(Bits {
            data: 0b011,
            len: 3,
        })
        .unwrap();
    for mut writer in [horizontal, vertical] {
        writer.write(fax::maps::EOL).unwrap();
        writer.write(fax::maps::EOL).unwrap();
        assert_rejected(1, &writer.finish(), "/K -1");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 参照行が変わる任意の3画素行を符号化し、全画素と極性を照合する。
    #[test]
    fn group_four_reference_rows_preserve_pixels(rows in prop::collection::vec(any::<[bool; 3]>(), 1..=32), inverted in any::<bool>()) {
        let mut encoder = fax::encoder::Encoder::new(VecWriter::new());
        for row in &rows {
            encoder.encode_line(row.iter().map(|&black| if black { fax::Color::Black } else { fax::Color::White }), 3).unwrap();
        }
        let data = encoder.finish().unwrap().finish();
        let pdf = fixture::ccitt_pdf_with_parameters(rows.len(), &data, &format!("/K -1 /BlackIs1 {inverted}"));
        let parsed = parse_pdf_bytes_with_options("group-four-model.pdf", &pdf, &ParseOptions { strict: false, ..Default::default() }).unwrap();
        prop_assert_eq!(&parsed.quality.issues, &vec![QualityIssue::OcrRequired { page: 0 }]);
        let png = &parsed.image_elements[0].png_bytes;
        let raw = pixels(png);
        let expected: Vec<u8> = rows.iter().flat_map(|row| std::iter::once(0).chain(row.iter().flat_map(|&black| [if black != inverted { 0 } else { 255 }; 3]))).collect();
        prop_assert_eq!(raw, expected);
    }
}

/// 既存の白黒交互fixtureを既定パラメータで保持する。
#[test]
fn group_four_default_parameters_preserve_black_and_white() {
    let pdf = fixture::ccitt_pdf(true, 2, &fixture::encoded_rows(true, 2));
    let parsed = parse_pdf_bytes_with_options(
        "group-four-default.pdf",
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
