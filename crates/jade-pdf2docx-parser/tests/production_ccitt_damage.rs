//! CCITTの破損行回復を独立画素モデルと公開品質診断で固定する。
#[allow(dead_code)]
#[path = "support/ccitt_pdf.rs"]
mod fixture;

use fax::{BitWriter, Bits, VecWriter};
use jade_pdf2docx_parser::{ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
use proptest::prelude::*;
use std::io::Read;

/// 0は黒、1は白、2は幅超過による破損行としてEOL付きで符号化する。
fn encoded(rows: &[u8], aligned: bool, rtc: bool) -> Vec<u8> {
    let mut writer = VecWriter::new();
    for &row in rows {
        if aligned {
            writer.pad();
        }
        writer.write(fax::maps::EOL).unwrap();
        if row == 0 {
            writer.write(fax::maps::white::encode(0).unwrap()).unwrap();
            writer.write(fax::maps::black::encode(3).unwrap()).unwrap();
        } else {
            writer
                .write(fax::maps::white::encode(if row == 1 { 3 } else { 4 }).unwrap())
                .unwrap();
        }
    }
    for _ in 0..if rtc { 6 } else { 1 } {
        if aligned {
            writer.pad();
        }
        writer.write(fax::maps::EOL).unwrap();
    }
    writer.finish()
}

/// PNGのfilter 0のRGB scanlineを独立に取り出す。
fn scanlines(png: &[u8]) -> Vec<u8> {
    let mut compressed = Vec::new();
    let mut offset = 8;
    while offset + 12 <= png.len() {
        let size = u32::from_be_bytes(png[offset..offset + 4].try_into().unwrap()) as usize;
        if &png[offset + 4..offset + 8] == b"IDAT" {
            compressed.extend_from_slice(&png[offset + 8..offset + 8 + size]);
        }
        offset += size + 12;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(compressed.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    raw
}

/// 前の正常行を複製し、連続破損・先頭破損は白へ戻す仕様の画素モデル。
fn expected(rows: &[u8], inverted: bool) -> Vec<u8> {
    let mut previous = 1;
    let mut raw = Vec::new();
    for &row in rows {
        let color = if row == 2 {
            if previous == 2 { 1 } else { previous }
        } else {
            row
        };
        // 補完した白行も正常な白runと同じBlackIs1の写像を通す。
        let gray = if (color == 0) != inverted { 0 } else { 255 };
        raw.push(0);
        raw.extend_from_slice(&[gray; 9]);
        previous = row;
    }
    raw
}

/// 復元画素・本文・警告・strict拒否を同じ公開入力で照合する。
fn assert_recovered(rows: &[u8], aligned: bool, rtc: bool, inverted: bool) {
    let damaged = rows.iter().filter(|&&row| row == 2).count();
    let data = encoded(rows, aligned, rtc);
    let parameters = format!(
        "/K 0 /EndOfLine true /EncodedByteAlign {aligned} /EndOfBlock {rtc} /BlackIs1 {inverted} /DamagedRowsBeforeError {damaged}"
    );
    let pdf = fixture::ccitt_pdf_with_parameters(rows.len(), &data, &parameters);
    let parsed =
        parse_pdf_bytes_with_options("recovered.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        scanlines(&parsed.image_elements[0].png_bytes),
        expected(rows, inverted)
    );
    assert!(parsed.text_preview.contains("retained text"));
    assert_eq!(parsed.quality.issues.len(), usize::from(damaged > 0) + 1);
    assert_eq!(
        parsed.quality.issues.last(),
        Some(&QualityIssue::OcrRequired { page: 0 })
    );
    if damaged > 0 {
        assert_eq!(
            parsed.quality.issues[0],
            QualityIssue::ImageRepaired {
                object_number: Some(4),
                damaged_rows: damaged as u64
            }
        );
        assert!(
            parsed.quality.issues[0]
                .to_string()
                .contains("image_repaired")
        );
        assert!(
            matches!(parse_pdf_bytes_with_options("recovered.pdf", &pdf, &ParseOptions { strict: true, ..Default::default() }), Err(ParseError::QualityRejected { report, .. }) if report == parsed.quality)
        );
    }
}

/// 単発・連続・先頭・末尾破損と極性・alignment・RTCを組み合わせる。
#[test]
fn damaged_rows_preserve_pixels_and_report_repair() {
    for rows in [&[0, 2, 2, 1, 2, 0][..], &[2, 0, 2][..], &[2, 2, 1][..]] {
        for aligned in [false, true] {
            for rtc in [false, true] {
                for inverted in [false, true] {
                    assert_recovered(rows, aligned, rtc, inverted);
                }
            }
        }
    }
}

/// 正常行を挟んでも破損数の許容枠をリセットしない。
#[test]
fn damaged_row_limit_is_total_and_inclusive() {
    let rows = [0, 2, 1, 2, 0];
    let data = encoded(&rows, false, true);
    for limit in [0, 1] {
        let parameters = format!("/K 0 /EndOfLine true /DamagedRowsBeforeError {limit}");
        let pdf = fixture::ccitt_pdf_with_parameters(rows.len(), &data, &parameters);
        let parsed =
            parse_pdf_bytes_with_options("limit.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert!(
            parsed.quality.issues[0]
                .to_string()
                .contains("decode_failed")
        );
    }
    assert_recovered(&rows, false, true, false);
}

/// 正常な行にも修復警告を追加しない。
#[test]
fn undamaged_rows_do_not_report_repair() {
    assert_recovered(&[0, 1, 0], true, true, false);
}

/// 最終行のEOLがなければ、許容枠があっても破損行の終端を捏造しない。
#[test]
fn damaged_row_without_resynchronization_is_rejected() {
    let mut writer = VecWriter::new();
    writer.write(fax::maps::EOL).unwrap();
    writer.write(fax::maps::white::encode(4).unwrap()).unwrap();
    let pdf = fixture::ccitt_pdf_with_parameters(
        1,
        &writer.finish(),
        "/K 0 /EndOfLine true /EndOfBlock false /DamagedRowsBeforeError 1",
    );
    let parsed =
        parse_pdf_bytes_with_options("truncated.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert!(
        parsed.quality.issues[0]
            .to_string()
            .contains("decode_failed")
    );
}

/// 破損許容枠があっても必須のRTC終端を省略した画像は成功扱いにしない。
#[test]
fn recovery_does_not_replace_required_rtc() {
    let data = encoded(&[0, 2, 1], false, false);
    let pdf = fixture::ccitt_pdf_with_parameters(
        3,
        &data,
        "/K 0 /EndOfLine true /EndOfBlock true /DamagedRowsBeforeError 2",
    );
    let parsed =
        parse_pdf_bytes_with_options("missing-rtc.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert!(matches!(
        parsed.quality.issues.as_slice(),
        [
            QualityIssue::ImageReplaced {
                reason: jade_pdf2docx_parser::ImageReplacementReason::DecodeFailed,
                ..
            },
            QualityIssue::OcrRequired { page: 0 }
        ]
    ));
}

/// Rowsが未知でも終端まで検証し、補完の画素と診断を保持する。
#[test]
fn unknown_row_count_preserves_recovery() {
    for rtc in [false, true] {
        let data = encoded(&[0, 2, 1], false, rtc);
        let pdf = fixture::ccitt_pdf_with_dimensions(
            3,
            3,
            &data,
            &format!(
                "/K 0 /Columns 3 /Rows 0 /EndOfLine true /EndOfBlock {rtc} /DamagedRowsBeforeError 1"
            ),
        );
        let parsed =
            parse_pdf_bytes_with_options("unknown-rows.pdf", &pdf, &ParseOptions::default())
                .unwrap();
        assert_eq!(
            scanlines(&parsed.image_elements[0].png_bytes),
            expected(&[0, 2, 1], false)
        );
        assert_eq!(
            parsed.quality.issues,
            [
                QualityIssue::ImageRepaired {
                    object_number: Some(4),
                    damaged_rows: 1
                },
                QualityIssue::OcrRequired { page: 0 }
            ]
        );
    }
}

/// Group 4では破損行許容値を使わず正常画像をそのまま復号する。
#[test]
fn group_four_ignores_damaged_row_allowance() {
    let data = fixture::encoded_rows(true, 3);
    let pdf =
        fixture::ccitt_pdf_with_parameters(3, &data, "/K -1 /DamagedRowsBeforeError 4294967295");
    let parsed =
        parse_pdf_bytes_with_options("group-four.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert!(matches!(
        parse_pdf_bytes_with_options("group-four.pdf", &pdf, &ParseOptions { strict: true, ..Default::default() }),
        Err(ParseError::QualityRejected { report, .. }) if report == parsed.quality
    ));
}

/// EndOfLine falseでは破損行許容を有効化しない。
#[test]
fn recovery_requires_explicit_end_of_line() {
    let data = encoded(&[0, 2, 0], false, true);
    let pdf = fixture::ccitt_pdf_with_parameters(
        3,
        &data,
        "/K 0 /EndOfLine false /DamagedRowsBeforeError 1",
    );
    let parsed =
        parse_pdf_bytes_with_options("no-eol.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert!(
        parsed.quality.issues[0]
            .to_string()
            .contains("decode_failed")
    );
}

/// 破損許容枠の間接値も同じ復元画素へ接続する。
#[test]
fn indirect_allowance_recovers_rows() {
    let rows = [0, 2, 0];
    let data = encoded(&rows, true, true);
    let pdf = fixture::ccitt_pdf_with_objects(
        3,
        3,
        &data,
        "/K 0 /Columns 3 /Rows 3 /EndOfLine true /EncodedByteAlign true /DamagedRowsBeforeError 7 0 R",
        &[b"1"],
    );
    let parsed =
        parse_pdf_bytes_with_options("indirect.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        scanlines(&parsed.image_elements[0].png_bytes),
        expected(&rows, false)
    );
    assert!(
        parsed.quality.issues[0]
            .to_string()
            .contains("image_repaired")
    );
}

/// 正常な幅の後のゴミも次行の破損として誤って一行ずらさない。
#[test]
fn damaged_tail_after_complete_run_recovers_current_row() {
    let mut writer = VecWriter::new();
    writer.write(fax::maps::EOL).unwrap();
    writer.write(fax::maps::white::encode(3).unwrap()).unwrap();
    writer
        .write(Bits {
            data: 0b1111,
            len: 4,
        })
        .unwrap();
    writer.write(fax::maps::EOL).unwrap();
    writer.write(fax::maps::white::encode(0).unwrap()).unwrap();
    writer.write(fax::maps::black::encode(3).unwrap()).unwrap();
    let pdf = fixture::ccitt_pdf_with_parameters(
        2,
        &writer.finish(),
        "/K 0 /EndOfLine true /EndOfBlock false /DamagedRowsBeforeError 1",
    );
    let parsed = parse_pdf_bytes_with_options("tail.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        scanlines(&parsed.image_elements[0].png_bytes),
        expected(&[2, 0], false)
    );
    assert!(
        parsed.quality.issues[0]
            .to_string()
            .contains("image_repaired")
    );
}

/// 途中まで保存した白runを捨て、直前の正常な黒行で行全体を補完する。
#[test]
fn partial_scanline_is_discarded_before_repair() {
    let mut writer = VecWriter::new();
    writer.write(fax::maps::EOL).unwrap();
    writer.write(fax::maps::white::encode(0).unwrap()).unwrap();
    writer.write(fax::maps::black::encode(3).unwrap()).unwrap();
    writer.write(fax::maps::EOL).unwrap();
    writer.write(fax::maps::white::encode(1).unwrap()).unwrap();
    writer.write(fax::maps::black::encode(3).unwrap()).unwrap();
    writer.write(fax::maps::EOL).unwrap();
    writer.write(fax::maps::white::encode(3).unwrap()).unwrap();
    for _ in 0..6 {
        writer.write(fax::maps::EOL).unwrap();
    }
    let pdf = fixture::ccitt_pdf_with_parameters(
        3,
        &writer.finish(),
        "/K 0 /EndOfLine true /DamagedRowsBeforeError 1",
    );
    let parsed =
        parse_pdf_bytes_with_options("partial.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        scanlines(&parsed.image_elements[0].png_bytes),
        expected(&[0, 2, 1], false)
    );
    assert_eq!(
        parsed.quality.issues,
        [
            QualityIssue::ImageRepaired {
                object_number: Some(4),
                damaged_rows: 1
            },
            QualityIssue::OcrRequired { page: 0 }
        ]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 破損位置と連続長が変わっても仕様の画素モデルと診断が一致する。
    #[test]
    fn recovery_matches_row_model(rows in prop::collection::vec(0u8..3, 1..12), aligned in any::<bool>(), rtc in any::<bool>(), inverted in any::<bool>()) {
        assert_recovered(&rows, aligned, rtc, inverted);
    }
}
