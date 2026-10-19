//! CCITTの省略寸法とRowsより優先する終端を公開APIで検査する。
#[path = "support/ccitt_pdf.rs"]
mod fixture;
use fax::{BitWriter, VecWriter};
use jade_pdf2docx_parser::{ParseOptions, parse_pdf_bytes_with_options};
use proptest::prelude::*;
use std::io::Read;

/// PNGの寸法とfilter 0の全行を比較して画素欠落を検出する。
fn assert_pixels(pdf: &[u8], width: usize, expected_rows: &[u8]) {
    let parsed = parse_pdf_bytes_with_options(
        "ccitt-dimensions.pdf",
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
    let mut idat = Vec::new();
    let mut cursor = 8;
    while cursor + 12 <= png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
        }
        cursor += length + 12;
    }
    let mut pixels = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut pixels)
        .unwrap();
    let expected: Vec<_> = expected_rows
        .iter()
        .flat_map(|&gray| std::iter::once(0).chain(std::iter::repeat_n(gray, width * 3)))
        .collect();
    assert_eq!(pixels, expected);
}

/// Rows 0と省略時にも、画像が要求する全行を保持する。
#[test]
fn unknown_rows_preserve_all_image_rows() {
    for group_four in [false, true] {
        let data = fixture::encoded_rows(group_four, 4);
        assert_pixels(
            &fixture::ccitt_pdf(group_four, 4, &data),
            3,
            &[0, 255, 0, 255],
        );
        let k = if group_four { -1 } else { 0 };
        assert_pixels(
            &fixture::ccitt_pdf_with_parameters(4, &data, &format!("/K {k}")),
            3,
            &[0, 255, 0, 255],
        );
        for rows in ["/Rows 0", ""] {
            let pdf = fixture::ccitt_pdf_with_dimensions(
                3,
                4,
                &fixture::encoded_rows(group_four, 4),
                &format!("/K {k} /Columns 3 {rows}"),
            );
            assert_pixels(&pdf, 3, &[0, 255, 0, 255]);
        }
    }
}

/// Rows 0と終端不要を組み合わせても複数行を1行へ縮めない。
#[test]
fn unknown_rows_without_end_block_preserve_image_height() {
    for (k, data) in [(-1, &[0xf0][..]), (0, &[0x88, 0x88][..])] {
        for rows in ["/Rows 0", ""] {
            let pdf = fixture::ccitt_pdf_with_dimensions(
                3,
                4,
                data,
                &format!("/K {k} /Columns 3 {rows} /EndOfBlock false"),
            );
            assert_pixels(&pdf, 3, &[255; 4]);
        }
    }
}

/// EndOfBlock trueではRowsの短い・長い宣言を画像の高さへ流用しない。
#[test]
fn end_block_overrides_declared_rows() {
    for group_four in [false, true] {
        let k = if group_four { -1 } else { 0 };
        for rows in [1, 5, 65_536] {
            let pdf = fixture::ccitt_pdf_with_dimensions(
                3,
                4,
                &fixture::encoded_rows(group_four, 4),
                &format!("/K {k} /Columns 3 /Rows {rows}"),
            );
            assert_pixels(&pdf, 3, &[0, 255, 0, 255]);
        }
    }
}

/// Columns省略時は仕様の1728画素として復号する。
#[test]
fn omitted_columns_use_standard_width() {
    for group_four in [false, true] {
        let mut writer = VecWriter::new();
        for _ in 0..3 {
            if group_four {
                writer
                    .write(fax::maps::mode::encode(fax::maps::Mode::Vertical(0)).unwrap())
                    .unwrap();
            } else {
                writer.write(fax::maps::EOL).unwrap();
                writer
                    .write(fax::maps::white::encode(1728).unwrap())
                    .unwrap();
                writer.write(fax::maps::white::encode(0).unwrap()).unwrap();
            }
        }
        for _ in 0..if group_four { 2 } else { 6 } {
            writer.write(fax::maps::EOL).unwrap();
        }
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(
            1728,
            3,
            &writer.finish(),
            &format!("/K {k} /Rows 0"),
        );
        assert_pixels(&pdf, 1728, &[255; 3]);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 終端優先と未知行数を、独立encoderの全画素モデルで確認する。
    #[test]
    fn end_block_height_model(group_four in any::<bool>(), height in 1usize..24, rows in 0usize..32) {
        let k = if group_four { -1 } else { 0 };
        let pdf = fixture::ccitt_pdf_with_dimensions(3, height, &fixture::encoded_rows(group_four, height),
            &format!("/K {k} /Columns 3 /Rows {rows}"));
        let expected: Vec<_> = (0..height).map(|row| if row % 2 == 0 {0} else {255}).collect();
        assert_pixels(&pdf, 3, &expected);
    }
}
