//! CCITTパラメータの型・範囲と間接値を公開APIで検査する。
#[path = "support/ccitt_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Read;

/// 不正パラメータを画像代替として報告し、strictで同じ診断を拒否する。
fn assert_rejected(pdf: &[u8]) {
    let normal =
        parse_pdf_bytes_with_options("ccitt-types.pdf", pdf, &ParseOptions::default()).unwrap();
    assert!(normal.text_preview.contains("retained text"));
    assert_eq!(normal.image_elements.len(), 1);
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
        matches!(parse_pdf_bytes_with_options("ccitt-types.pdf", pdf,
        &ParseOptions { strict: true, ..Default::default() }),
        Err(ParseError::QualityRejected { report, .. }) if report == normal.quality)
    );
}

/// 正常値が全画素と本文を保持することを確認する。
fn assert_pixels(pdf: &[u8], width: usize, rows: &[u8]) {
    let parsed = parse_pdf_bytes_with_options(
        "ccitt-types.pdf",
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
        rows.len()
    );
    let mut idat = Vec::new();
    let mut cursor = 8;
    while cursor + 12 <= png.len() {
        let size = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + size]);
        }
        cursor += size + 12;
    }
    let mut pixels = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut pixels)
        .unwrap();
    let expected: Vec<_> = rows
        .iter()
        .flat_map(|&gray| std::iter::once(0).chain(std::iter::repeat_n(gray, width * 3)))
        .collect();
    assert_eq!(pixels, expected);
}

/// 各整数属性のreal・name・負値を丸めたり既定値へ置換しない。
#[test]
fn invalid_integer_parameters_are_diagnosed() {
    let cases = [
        "/K -0.5 /Columns 3 /Rows 4",
        "/K /Unknown /Columns 3 /Rows 4",
        "/K 0 /Columns 3.5 /Rows 4",
        "/K 0 /Columns (3) /Rows 4",
        "/K 0 /Columns 3 /Rows -1",
        "/K 0 /Columns 3 /Rows 4.5",
        "/K 0 /Columns 3 /Rows 4294967296",
        "/K 0 /Columns 3 /Rows /Unknown",
        "/K 0 /Columns 3 /Rows 4 /DamagedRowsBeforeError -1",
        "/K 0 /Columns 3 /Rows 4 /DamagedRowsBeforeError 0.5",
    ];
    let data = fixture::encoded_rows(false, 4);
    for parameters in cases {
        assert_rejected(&fixture::ccitt_pdf_with_dimensions(3, 4, &data, parameters));
    }
}

/// boolean以外の値をfalse/trueの既定値に変えて成功扱いにしない。
#[test]
fn invalid_boolean_parameters_are_diagnosed() {
    let data = fixture::encoded_rows(false, 4);
    for key in ["BlackIs1", "EndOfBlock", "EndOfLine", "EncodedByteAlign"] {
        for value in [
            "0",
            "1",
            "/true",
            "(false)",
            "[false]",
            "<< /Value false >>",
        ] {
            let parameters = format!("/K 0 /Columns 3 /Rows 4 /{key} {value}");
            assert_rejected(&fixture::ccitt_pdf_with_dimensions(
                3,
                4,
                &data,
                &parameters,
            ));
        }
    }
}

/// 参照先の実数・boolean誤型にも直接値と同じ検証を適用する。
#[test]
fn invalid_indirect_parameters_are_diagnosed() {
    let data = fixture::encoded_rows(true, 4);
    for (parameters, value) in [
        ("/K -1 /Columns 3 /Rows 7 0 R", b"4.0".as_slice()),
        ("/K -1 /Columns 3 /EndOfBlock 7 0 R", b"1".as_slice()),
        (
            "/K -1 /Columns 3 /DamagedRowsBeforeError 7 0 R",
            b"-1".as_slice(),
        ),
    ] {
        assert_rejected(&fixture::ccitt_pdf_with_objects(
            3,
            4,
            &data,
            parameters,
            &[value],
        ));
    }
}

/// 零・負のColumnsを1画素へclampせず拒否する。
#[test]
fn nonpositive_columns_are_diagnosed() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    encoder
        .encode_line([fax::Color::White].into_iter(), 1)
        .unwrap();
    let data = encoder.finish().unwrap().finish();
    for columns in ["0", "-1"] {
        assert_rejected(&fixture::ccitt_pdf_with_dimensions(
            3,
            1,
            &data,
            &format!("/K -1 /Columns {columns}"),
        ));
    }
}

/// 全属性の間接scalarを参照番号ではなく値として解釈する。
#[test]
fn indirect_parameters_preserve_pixels() {
    for group_four in [false, true] {
        let data = fixture::encoded_rows(group_four, 4);
        let k = if group_four {
            b"-1".as_slice()
        } else {
            b"0".as_slice()
        };
        let pdf = fixture::ccitt_pdf_with_objects(
            3,
            4,
            &data,
            "/K 7 0 R /Columns 8 0 R /Rows 9 0 R /BlackIs1 10 0 R /EndOfBlock 11 0 R /EndOfLine 12 0 R /EncodedByteAlign 13 0 R /DamagedRowsBeforeError 14 0 R",
            &[
                k,
                b"% columns\n+3",
                b"4",
                b"false",
                b"true",
                b"false",
                b"false",
                b"0",
            ],
        );
        assert_pixels(&pdf, 3, &[0, 255, 0, 255]);
    }
}

/// escapeしたキーと入れ子の同名キーを字句のscopeに従って解釈する。
#[test]
fn escaped_and_scoped_parameters_preserve_pixels() {
    let data = fixture::encoded_rows(true, 4);
    assert_pixels(
        &fixture::ccitt_pdf_with_dimensions(
            3,
            4,
            &data,
            "/Ignored << /K 0 /Columns 1 /Rows 1 /BlackIs1 true >> /#4b -1 /Col#75mns +3 /Ro#77s +4 /Black#49s1 false",
        ),
        3,
        &[0, 255, 0, 255],
    );
}

/// nullは属性省略と等価に扱い、明示整数とbooleanも保持する。
#[test]
fn null_and_explicit_parameters_preserve_pixels() {
    let data = fixture::encoded_rows(false, 4);
    assert_pixels(&fixture::ccitt_pdf(false, 4, &data), 3, &[0, 255, 0, 255]);
    assert_pixels(
        &fixture::ccitt_pdf_with_parameters(4, &data, "/K +0 /BlackIs1 false"),
        3,
        &[0, 255, 0, 255],
    );
    assert_pixels(
        &fixture::ccitt_pdf_with_dimensions(
            3,
            4,
            &data,
            "/K null /Columns +3 /Rows null /BlackIs1 null /EndOfBlock null /EndOfLine null /EncodedByteAlign null /DamagedRowsBeforeError null",
        ),
        3,
        &[0, 255, 0, 255],
    );
    assert_pixels(
        &fixture::ccitt_pdf_with_objects(
            3,
            4,
            &data,
            "/Columns 3 /Rows 7 0 R /EndOfBlock 7 0 R",
            &[b"null"],
        ),
        3,
        &[0, 255, 0, 255],
    );
}

/// nullのColumnsも省略時の1728を使い、BlackIs1のtrueを正確に反映する。
#[test]
fn null_columns_and_true_boolean_preserve_pixels() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    encoder
        .encode_line(std::iter::repeat_n(fax::Color::White, 1728), 1728)
        .unwrap();
    let data = encoder.finish().unwrap().finish();
    assert_pixels(
        &fixture::ccitt_pdf_with_dimensions(1728, 1, &data, "/K -1 /Columns null /BlackIs1 true"),
        1728,
        &[0],
    );
    let data = fixture::encoded_rows(false, 4);
    assert_pixels(
        &fixture::ccitt_pdf_with_objects(
            3,
            4,
            &data,
            "/K +0 /Columns +3 /Rows +4 /BlackIs1 7 0 R /EndOfLine true /EndOfBlock false",
            &[b"true"],
        ),
        3,
        &[255, 0, 255, 0],
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 整数Rowsを実数へ変更すると常に拒否し、暗黙の切り捨てを許さない。
    #[test]
    fn real_rows_are_never_truncated(rows in 0u32..32, fractional in 0u32..10) {
        let data = fixture::encoded_rows(true, 4);
        let pdf = fixture::ccitt_pdf_with_dimensions(3, 4, &data,
            &format!("/K -1 /Columns 3 /Rows {rows}.{fractional}"));
        assert_rejected(&pdf);
    }
}
