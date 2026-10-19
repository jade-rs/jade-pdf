//! CCITTの復号byte配置と画像辞書の寸法を独立に検査する。
#[path = "support/ccitt_pdf.rs"]
#[allow(dead_code)]
mod fixture;
use fax::{BitWriter, Color, VecWriter};
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Read;

/// 全画素が既知の行を独立encoderまたは1D符号表で符号化する。
fn encoded(group_four: bool, columns: u16, rows: &[bool]) -> Vec<u8> {
    if group_four {
        let mut encoder = fax::encoder::Encoder::new(VecWriter::new());
        for &black in rows {
            encoder
                .encode_line(
                    std::iter::repeat_n(
                        if black { Color::Black } else { Color::White },
                        columns as usize,
                    ),
                    columns,
                )
                .unwrap();
        }
        return encoder.finish().unwrap().finish();
    }
    let mut writer = VecWriter::new();
    for &black in rows {
        writer.write(fax::maps::EOL).unwrap();
        if black {
            writer.write(fax::maps::white::encode(0).unwrap()).unwrap();
            writer
                .write(fax::maps::black::encode(columns).unwrap())
                .unwrap();
        } else {
            writer
                .write(fax::maps::white::encode(columns).unwrap())
                .unwrap();
        }
    }
    for _ in 0..6 {
        writer.write(fax::maps::EOL).unwrap();
    }
    writer.finish()
}

/// strict成功・本文・PNG寸法と非圧縮の全sampleを検査する。
fn assert_pixels(pdf: &[u8], width: usize, height: usize, expected: &[u8]) {
    let parsed = parse_pdf_bytes_with_options(
        "ccitt-layout.pdf",
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
        height
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
    let expected: Vec<u8> = expected
        .chunks_exact(width)
        .flat_map(|row| std::iter::once(0).chain(row.iter().flat_map(|&gray| [gray; 3])))
        .collect();
    assert_eq!(actual, expected);
}

/// 必要byteが不足する画像を代替診断し、strictでは同じreportで拒否する。
fn assert_rejected(pdf: &[u8]) {
    let normal =
        parse_pdf_bytes_with_options("ccitt-layout.pdf", pdf, &ParseOptions::default()).unwrap();
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
        matches!(parse_pdf_bytes_with_options("ccitt-layout.pdf", pdf,
        &ParseOptions { strict: true, ..Default::default() }),
        Err(ParseError::QualityRejected { report, .. }) if report == normal.quality)
    );
}

/// 同じpacked strideの幅差では、画像のWidthまでのsampleだけを描画する。
#[test]
fn narrower_image_preserves_its_dimensions() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            1,
            4,
            &encoded(group_four, 3, &[true, false, true, false]),
            &format!("/K {k} /Columns 3 /Rows 4"),
        );
        assert_pixels(&pdf, 1, 4, &[0, 255, 0, 255]);
    }
}

/// 終端不要の正のRowsはfilterの高さであり、PNGの高さには流用しない。
#[test]
fn declared_filter_rows_do_not_change_image_height() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            3,
            2,
            &encoded(group_four, 3, &[true, false, true, false]),
            &format!("/K {k} /Columns 3 /Rows 4 /EndOfBlock false"),
        );
        assert_pixels(&pdf, 3, 2, &[0, 0, 0, 255, 255, 255]);
    }
}

/// filterの1行から画像の複数行がbyteごとに読み取られる。
#[test]
fn image_rows_can_share_a_filter_row() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            3,
            2,
            &encoded(group_four, 16, &[true]),
            &format!("/K {k} /Columns 16 /Rows 0"),
        );
        assert_pixels(&pdf, 3, 2, &[0; 6]);
    }
}

/// 画像の1行がfilterの複数行をまたいでもsample順序を維持する。
#[test]
fn image_row_can_span_filter_rows() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        for end_block in [false, true] {
            let pdf = fixture::ccitt_pdf_with_dimensions(
                9,
                2,
                &encoded(group_four, 8, &[true, false, false, true]),
                &format!("/K {k} /Columns 8 /Rows 4 /EndOfBlock {end_block}"),
            );
            let expected: Vec<_> = [vec![0; 8], vec![255; 9], vec![0]].concat();
            assert_pixels(&pdf, 9, 2, &expected);
        }
    }
}

/// 画像の最後の行がfilterの最終行を使い切らなくても寸法を膨らませない。
#[test]
fn partial_final_filter_row_preserves_image_height() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            8,
            3,
            &encoded(group_four, 16, &[true, false]),
            &format!("/K {k} /Columns 16 /Rows 0"),
        );
        assert_pixels(&pdf, 8, 3, &[vec![0; 16], vec![255; 8]].concat());
    }
}

/// 完全なfilterでも画像に必要なbyte数が不足する場合は成功扱いにしない。
#[test]
fn insufficient_decoded_bytes_are_diagnosed() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            3,
            4,
            &encoded(group_four, 3, &[true, false]),
            &format!("/K {k} /Columns 3 /Rows 2 /EndOfBlock false"),
        );
        assert_rejected(&pdf);
    }
}

/// filter末尾の未使用bitは白として扱い、極性を反転しても位置を変えない。
#[test]
fn filter_row_padding_is_preserved_with_polarity() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        for inverted in [false, true] {
            let pdf = fixture::ccitt_pdf_with_dimensions(
                8,
                2,
                &encoded(group_four, 3, &[true, false]),
                &format!("/K {k} /Columns 3 /Rows 2 /BlackIs1 {inverted}"),
            );
            let mut expected = [vec![0; 3], vec![255; 13]].concat();
            if inverted {
                expected.iter_mut().for_each(|gray| *gray = 255 - *gray);
            }
            assert_pixels(&pdf, 8, 2, &expected);
        }
    }
}

/// 終端不要・未知行数でも画像側のbyte数から必要な複数filter行を求める。
#[test]
fn unknown_rows_without_terminal_use_packed_stride() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            9,
            2,
            &encoded(group_four, 8, &[true, false, false, true]),
            &format!("/K {k} /Columns 8 /Rows 0 /EndOfBlock false"),
        );
        assert_pixels(&pdf, 9, 2, &[vec![0; 8], vec![255; 9], vec![0]].concat());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// packed byte列の独立モデルで幅・高さ・極性とrow strideの組合せを検査する。
    #[test]
    fn packed_samples_model(group_four in any::<bool>(), width in 1usize..=24,
        height in 1usize..=10, wide in any::<bool>(), inverted in any::<bool>(), end_block in any::<bool>()) {
        let columns = if wide { 16usize } else { 8 };
        let stride = columns / 8;
        let image_stride = width.div_ceil(8);
        let rows = (image_stride * height).div_ceil(stride);
        let black: Vec<_> = (0..rows).map(|row| row % 3 == 0).collect();
        let samples: Vec<_> = black.iter().flat_map(|&black|
            std::iter::repeat_n(if black != inverted { 0 } else { 255 }, columns)).collect();
        let expected: Vec<_> = (0..height).flat_map(|row|
            samples[row * image_stride * 8..row * image_stride * 8 + width].iter().copied()).collect();
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(width, height, &encoded(group_four, columns as u16, &black),
            &format!("/K {k} /Columns {columns} /Rows {rows} /EndOfBlock {end_block} /BlackIs1 {inverted}"));
        assert_pixels(&pdf, width, height, &expected);
    }
}

/// 必要sampleの後も終端まで正常行を検査し、画像の先頭sampleだけを描画する。
#[test]
fn terminal_overrides_minimum_sample_rows() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        for declared in [0, 1, 99] {
            let pdf = fixture::ccitt_pdf_with_dimensions(
                3,
                1,
                &encoded(group_four, 3, &[true, false, true, false]),
                &format!("/K {k} /Columns 3 /Rows {declared}"),
            );
            assert_pixels(&pdf, 3, 1, &[0, 0, 0]);
        }
    }
}

/// 画像の必要行を満たしても、後続の壊れた行を黙って捨てない。
#[test]
fn malformed_extra_rows_are_diagnosed() {
    for group_four in [false, true] {
        let mut data = encoded(group_four, 3, &[true, false, true, false]);
        data.truncate(data.len() / 2);
        let k = if group_four { -1 } else { 0 };
        let pdf =
            fixture::ccitt_pdf_with_dimensions(3, 1, &data, &format!("/K {k} /Columns 3 /Rows 1"));
        assert_rejected(&pdf);
    }
}

/// 追加行の画素予算を検査し、画像の高さで打ち切らない。
#[test]
fn extra_rows_obey_pixel_limit() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            3,
            1,
            &encoded(group_four, 3, &[true, false, true, false]),
            &format!("/K {k} /Columns 3 /Rows 1"),
        );
        let result = parse_pdf_bytes_with_options(
            "ccitt-limit.pdf",
            &pdf,
            &ParseOptions {
                limits: jade_pdf2docx_parser::ParseLimits {
                    max_image_pixels: 9,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert!(
            matches!(
                result,
                Err(ParseError::ResourceLimit {
                    resource: "image pixels",
                    limit: 9,
                    actual: 12
                })
            ),
            "{result:?}"
        );
    }
}

/// 未知行数の高さにu16制限を持ち込まず、終端まで検査する。
#[test]
fn terminal_rows_can_exceed_u16() {
    let pdf = fixture::ccitt_pdf_with_dimensions(
        1,
        1,
        &encoded(true, 1, &vec![false; 65_536]),
        "/K -1 /Columns 1 /Rows 0",
    );
    assert_pixels(&pdf, 1, 1, &[255]);
}

/// 行内の色境界も独立encoderで符号化する。
fn encoded_pixels(group_four: bool, rows: &[Vec<bool>]) -> Vec<u8> {
    let columns = rows[0].len() as u16;
    if group_four {
        let mut encoder = fax::encoder::Encoder::new(VecWriter::new());
        for row in rows {
            encoder
                .encode_line(
                    row.iter()
                        .map(|&black| if black { Color::Black } else { Color::White }),
                    columns,
                )
                .unwrap();
        }
        return encoder.finish().unwrap().finish();
    }
    let mut writer = VecWriter::new();
    for row in rows {
        writer.write(fax::maps::EOL).unwrap();
        let mut position = 0;
        let mut black = false;
        while position < row.len() {
            let length = row[position..]
                .iter()
                .take_while(|&&pixel| pixel == black)
                .count();
            writer
                .write(
                    if black {
                        fax::maps::black::encode(length as u16)
                    } else {
                        fax::maps::white::encode(length as u16)
                    }
                    .unwrap(),
                )
                .unwrap();
            position += length;
            black = !black;
        }
    }
    for _ in 0..6 {
        writer.write(fax::maps::EOL).unwrap();
    }
    writer.finish()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 色境界のあるpacked byteを分割・連結し、追加行を含めてモデルと比較する。
    #[test]
    fn variable_rows_preserve_nonuniform_samples(
        group_four in any::<bool>(), width in 1usize..=24, height in 1usize..=6,
        wide in any::<bool>(), extra in 1usize..=5, inverted in any::<bool>(),
        seed in any::<u16>()
    ) {
        let columns = if wide { 16 } else { 8 };
        let image_stride = width.div_ceil(8);
        let required = (image_stride * height).div_ceil(columns / 8);
        let rows: Vec<Vec<bool>> = (0..required + extra).map(|row|
            (0..columns).map(|x| (seed >> ((row * columns + x) % 16)) & 1 != 0).collect()
        ).collect();
        let samples: Vec<bool> = rows.iter().flatten().copied().collect();
        let expected: Vec<u8> = (0..height).flat_map(|row| (0..width).map({
            let samples = &samples;
            move |x| if samples[row * image_stride * 8 + x] != inverted { 0 } else { 255 }
        })).collect();
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(width, height,
            &encoded_pixels(group_four, &rows),
            &format!("/K {k} /Columns {columns} /Rows 1 /BlackIs1 {inverted}"));
        assert_pixels(&pdf, width, height, &expected);
    }
}

/// 正常な終端でも画像に必要なpacked byteが不足すれば拒否する。
#[test]
fn early_terminal_does_not_fill_missing_samples() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            9,
            2,
            &encoded(group_four, 8, &[true, false, true]),
            &format!("/K {k} /Columns 8 /Rows 99"),
        );
        assert_rejected(&pdf);
    }
}

/// 必要sampleの後の過大runを、完全な終端が続いても拒否する。
#[test]
fn oversized_extra_row_is_diagnosed() {
    for group_four in [false, true] {
        let mut writer = VecWriter::new();
        for width in [3, 4] {
            if group_four {
                writer
                    .write(fax::maps::mode::encode(fax::maps::Mode::Horizontal).unwrap())
                    .unwrap();
            } else {
                writer.write(fax::maps::EOL).unwrap();
            }
            writer.write(fax::maps::white::encode(0).unwrap()).unwrap();
            writer
                .write(fax::maps::black::encode(width).unwrap())
                .unwrap();
        }
        for _ in 0..if group_four { 2 } else { 6 } {
            writer.write(fax::maps::EOL).unwrap();
        }
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            3,
            1,
            &writer.finish(),
            &format!("/K {k} /Columns 3 /Rows 1"),
        );
        assert_rejected(&pdf);
    }
}
