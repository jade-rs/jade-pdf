//! ReportLab標準のASCII85→Flate画像を画素・予算・品質で検証する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{ParseError, ParseLimits, ParseOptions, parse_pdf_bytes_with_options};

/// 独立した除算encoderで、終端と途中のPDF空白を持つテスト入力を作る。
fn encoded(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for group in data.chunks(4) {
        let mut padded = [0; 4];
        padded[..group.len()].copy_from_slice(group);
        let mut value = u32::from_be_bytes(padded);
        let mut digits = [0; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        result.extend_from_slice(&digits[..group.len() + 1]);
        result.extend_from_slice(b" \n\t\0\r\x0c");
    }
    result.extend_from_slice(b"~>");
    result
}

/// 任意RGB画素とPNG予測値を、単一Flateと同じ画素へ復元する。
#[test]
fn ascii85_flate_pixels_match_direct_flate_property() {
    use proptest::prelude::*;
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 32,
        ..Default::default()
    });
    runner.run(&(any::<[u8; 6]>(), any::<bool>()), |(samples, predicted)| {
        let mut source = Vec::new();
        if predicted { source.push(0); }
        source.extend_from_slice(&samples);
        let data = fixture::compressed(&source);
        let parameters = if predicted { "<< /Predictor 15 /Columns 2 /Colors 3 >>" } else { "null" };
        let attrs = "/ColorSpace /DeviceRGB /Decode [1 0 1 0 1 0]";
        let direct = fixture::source_image_pdf(&format!("{attrs} /Filter /FlateDecode /DecodeParms {parameters}"), &data, "");
        let chain = fixture::source_image_pdf(&format!("{attrs} /Filter [/ASCII85Decode /FlateDecode] /DecodeParms [null {parameters}]"), &encoded(&data), "");
        let direct = parse_pdf_bytes_with_options("direct.pdf", &direct, &ParseOptions::default()).unwrap();
        let chain = parse_pdf_bytes_with_options("chain.pdf", &chain, &ParseOptions::default()).unwrap();
        prop_assert_eq!(&chain.quality.issues, &direct.quality.issues);
        prop_assert_eq!(&chain.image_elements[0].png_bytes, &direct.image_elements[0].png_bytes);
        Ok(())
    }).unwrap();
}

/// ASCII85の復号量も単一streamと解析累積の両予算へ数える。
#[test]
fn ascii85_stage_obeys_stream_and_total_budgets() {
    let data = fixture::compressed(&[30, 60, 90, 120, 150, 180]);
    let pdf = fixture::source_image_pdf(
        "/ColorSpace /DeviceRGB /Filter [/ASCII85Decode /FlateDecode]",
        &encoded(&data),
        "",
    );
    for (limits, resource) in [
        (
            ParseLimits {
                max_decoded_stream_bytes: data.len() as u64 - 1,
                ..Default::default()
            },
            "decoded stream bytes",
        ),
        (
            ParseLimits {
                max_total_decoded_bytes: data.len() as u64 - 1,
                ..Default::default()
            },
            "total decoded bytes",
        ),
    ] {
        assert!(
            matches!(parse_pdf_bytes_with_options("budget.pdf", &pdf, &ParseOptions { limits, ..Default::default() }),
            Err(ParseError::ResourceLimit { resource: actual, .. }) if actual == resource)
        );
    }
}

/// 壊れたASCII85や非対応の段別パラメータをstrictで拒否する。
#[test]
fn malformed_ascii85_and_outer_parameters_are_not_ignored() {
    for (data, parameters) in [
        (b"!~>".as_slice(), ""),
        (b"uuuuu~>", ""),
        (b"!z~>", ""),
        (b"!!!!!", ""),
        (b"!!!!!~x", ""),
        (b"!!!!!~>", "/DecodeParms [<< /Predictor 2 >> null]"),
    ] {
        let pdf = fixture::source_image_pdf(
            &format!("/ColorSpace /DeviceRGB /Filter [/ASCII85Decode /FlateDecode] {parameters}"),
            data,
            "",
        );
        assert!(matches!(
            parse_pdf_bytes_with_options(
                "invalid.pdf",
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
