//! 終端不要のCCITTで、入力末尾・正常行・画像sample不足を公開APIで区別する。
#[path = "support/ccitt_pdf.rs"]
#[allow(dead_code)]
mod fixture;
use fax::{BitWriter, Bits, VecWriter};
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Read;

/// 各行の1D runを独立に符号化し、Group 4ではHorizontalで参照行から独立させる。
fn write_rows(writer: &mut VecWriter, group_four: bool, rows: usize, eol: bool, aligned: bool) {
    for row in 0..rows {
        if aligned {
            writer.pad();
        }
        if eol {
            writer.write(fax::maps::EOL).unwrap();
        }
        let black = row % 2 == 0;
        if group_four {
            writer
                .write(Bits {
                    data: 0b001,
                    len: 3,
                })
                .unwrap();
        }
        writer
            .write(fax::maps::white::encode(if black { 0 } else { 3 }).unwrap())
            .unwrap();
        if black || group_four {
            writer
                .write(fax::maps::black::encode(if black { 3 } else { 0 }).unwrap())
                .unwrap();
        }
    }
}

/// 3画素幅の画像と未知または正のRowsを持つ正式xref付きPDFを作る。
fn pdf(group_four: bool, height: usize, data: &[u8], parameters: &str) -> Vec<u8> {
    fixture::ccitt_pdf_with_dimensions(
        3,
        height,
        data,
        &format!(
            "/K {} /Columns 3 /EndOfBlock false {parameters}",
            if group_four { -1 } else { 0 }
        ),
    )
}

/// 本文とPNGの全画素を検査し、行数の水増しを見逃さない。
fn assert_pixels(pdf: &[u8], rows: usize) {
    let expected: Vec<u8> = (0..rows)
        .map(|row| if row % 2 == 0 { 0 } else { 255 })
        .collect();
    assert_row_pixels(pdf, 3, &expected);
}

/// 指定した均一行モデルとPNGの寸法・全sampleを照合する。
fn assert_row_pixels(pdf: &[u8], width: usize, expected_rows: &[u8]) {
    let parsed = parse_pdf_bytes_with_options(
        "ccitt-exhaustion.pdf",
        pdf,
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
    let png = &parsed.image_elements[0].png_bytes;
    assert_eq!(
        u32::from_be_bytes(png[16..20].try_into().unwrap()) as usize,
        width
    );
    assert_eq!(
        u32::from_be_bytes(png[20..24].try_into().unwrap()) as usize,
        expected_rows.len()
    );
    let mut compressed = Vec::new();
    let mut cursor = 8;
    while cursor + 12 <= png.len() {
        let size = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            compressed.extend_from_slice(&png[cursor + 8..cursor + 8 + size]);
        }
        cursor += size + 12;
    }
    let mut actual = Vec::new();
    flate2::read::ZlibDecoder::new(compressed.as_slice())
        .read_to_end(&mut actual)
        .unwrap();
    let expected: Vec<u8> = expected_rows
        .iter()
        .flat_map(|&gray| std::iter::once(0).chain(std::iter::repeat_n(gray, width * 3)))
        .collect();
    assert_eq!(actual, expected);
}

/// 破損を通常時の画像代替とstrictの同一診断で検査する。
fn assert_rejected(pdf: &[u8]) {
    let normal =
        parse_pdf_bytes_with_options("ccitt-exhaustion.pdf", pdf, &ParseOptions::default())
            .unwrap();
    assert!(normal.text_preview.contains("retained text"));
    assert_eq!(
        normal.quality.issues,
        [
            QualityIssue::ImageReplaced {
                object_number: Some(4),
                reason: ImageReplacementReason::DecodeFailed,
            },
            QualityIssue::OcrRequired { page: 0 }
        ]
    );
    assert!(
        matches!(parse_pdf_bytes_with_options("ccitt-exhaustion.pdf", pdf,
        &ParseOptions { strict: true, ..Default::default() }),
        Err(ParseError::QualityRejected { report, .. }) if report == normal.quality)
    );
}

/// 正のRowsより早い正常な入力末尾でも、画像sampleが足りれば成功する。
#[test]
fn complete_input_can_end_before_declared_rows() {
    for group_four in [false, true] {
        for eol in [false, true] {
            for aligned in [false, true] {
                let mut writer = VecWriter::new();
                write_rows(&mut writer, group_four, 2, eol, aligned);
                let data = writer.finish();
                let parameters = format!("/Rows 9 /EndOfLine {eol} /EncodedByteAlign {aligned}");
                assert_pixels(&pdf(group_four, 2, &data, &parameters), 2);
                assert_rejected(&pdf(group_four, 3, &data, &parameters));
            }
        }
    }
}

/// 未知Rowsでは画像に必要な行数を超えた正常行も画素予算へ数える。
#[test]
fn unknown_rows_count_extra_rows_before_exhaustion() {
    for group_four in [false, true] {
        let mut writer = VecWriter::new();
        write_rows(&mut writer, group_four, 4, false, false);
        let data = writer.finish();
        for parameters in ["", "/Rows 0"] {
            let pdf = pdf(group_four, 1, &data, parameters);
            assert_pixels(&pdf, 1);
            let mut options = ParseOptions::default();
            options.limits.max_image_pixels = 9;
            assert!(matches!(
                parse_pdf_bytes_with_options("ccitt-budget.pdf", &pdf, &options),
                Err(ParseError::ResourceLimit {
                    resource: "image pixels",
                    actual: 12,
                    ..
                })
            ));
        }
    }
}

/// 画像sampleが足りた後でも、非ゼロの不完全mode/runと余分なゼロbyteを拒否する。
#[test]
fn unknown_rows_reject_malformed_extra_data() {
    for group_four in [false, true] {
        for suffix in [
            Bits { data: 0, len: 8 },
            Bits {
                data: 0b001,
                len: 3,
            },
        ] {
            let mut writer = VecWriter::new();
            write_rows(&mut writer, group_four, 1, false, false);
            writer.write(suffix).unwrap();
            assert_rejected(&pdf(group_four, 1, &writer.finish(), "/Rows 0"));
        }
    }
}

/// 行数到達後の符号列は読む必要がなく、従来の正のRows上限を保つ。
#[test]
fn declared_rows_stop_before_trailing_data() {
    for group_four in [false, true] {
        let mut writer = VecWriter::new();
        write_rows(&mut writer, group_four, 1, false, false);
        writer.write(Bits { data: 0, len: 16 }).unwrap();
        assert_pixels(&pdf(group_four, 1, &writer.finish(), "/Rows 1"), 1);
    }
}

/// 任意の終端patternは未知Rowsでも受け入れ、その後のbyteを復号しない。
#[test]
fn optional_terminal_ends_unknown_rows() {
    for group_four in [false, true] {
        for aligned in [false, true] {
            let mut writer = VecWriter::new();
            write_rows(&mut writer, group_four, 2, false, aligned);
            for marker in 0..if group_four { 2 } else { 6 } {
                // EOFBの2つのEOLは連続24bit、RTCは各EOLの前にalignmentできる。
                if aligned && (!group_four || marker == 0) {
                    writer.pad();
                }
                writer.write(fax::maps::EOL).unwrap();
            }
            writer.write(Bits { data: 0xff, len: 8 }).unwrap();
            assert_pixels(
                &pdf(
                    group_four,
                    2,
                    &writer.finish(),
                    &format!("/Rows 0 /EncodedByteAlign {aligned}"),
                ),
                2,
            );
        }
    }
}

/// 有効EOLの直後に入力が終わるとき、空の画像行を捏造しない。
#[test]
fn trailing_eol_can_end_input_without_another_row() {
    for group_four in [false, true] {
        for aligned in [false, true] {
            let mut writer = VecWriter::new();
            write_rows(&mut writer, group_four, 2, true, aligned);
            if aligned {
                writer.pad();
            }
            writer.write(fax::maps::EOL).unwrap();
            assert_pixels(
                &pdf(
                    group_four,
                    2,
                    &writer.finish(),
                    &format!("/Rows 9 /EndOfLine true /EncodedByteAlign {aligned}"),
                ),
                2,
            );
        }
    }
}

/// 未知Rowsでも行幅超過は画像に必要な行数の後まで診断する。
#[test]
fn oversized_extra_row_is_rejected() {
    for group_four in [false, true] {
        let mut writer = VecWriter::new();
        write_rows(&mut writer, group_four, 1, false, false);
        if group_four {
            writer
                .write(Bits {
                    data: 0b001,
                    len: 3,
                })
                .unwrap();
        }
        writer.write(fax::maps::white::encode(0).unwrap()).unwrap();
        writer.write(fax::maps::black::encode(4).unwrap()).unwrap();
        assert_rejected(&pdf(group_four, 1, &writer.finish(), "/Rows 0"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 独立1D runモデルで、未知Rows/早いEOFと任意の正常行境界を検査する。
    #[test]
    fn complete_rows_exhaustion_matches_model(
        group_four in any::<bool>(), rows in 1usize..17,
        eol in any::<bool>(), aligned in any::<bool>(), unknown in any::<bool>(),
    ) {
        let mut writer = VecWriter::new();
        write_rows(&mut writer, group_four, rows, eol, aligned);
        let parameters = format!("/Rows {} /EndOfLine {eol} /EncodedByteAlign {aligned}",
            if unknown { 0 } else { rows + 5 });
        assert_pixels(&pdf(group_four, rows, &writer.finish(), &parameters), rows);
    }
}

/// G4白行の1bit modeで、末尾のゼロpadding長0〜7を網羅する。
#[test]
fn every_byte_padding_length_is_accepted() {
    for rows in 1..=8 {
        let mut writer = VecWriter::new();
        for _ in 0..rows {
            writer.write(Bits { data: 1, len: 1 }).unwrap();
        }
        let pdf = fixture::ccitt_pdf_with_dimensions(
            1,
            rows,
            &writer.finish(),
            "/K -1 /Columns 1 /Rows 99 /EndOfBlock false",
        );
        assert_row_pixels(&pdf, 1, &vec![255; rows]);
    }
}
