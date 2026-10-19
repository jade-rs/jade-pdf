use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseLimits, ParseOptions, QualityIssue, parse_pdf_bytes,
    parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Write;

/// 画像 stream を持つ最小 PDF を生成する。
fn image_pdf(count: u32, width: u32, height: u32, filter: &str, data: &[u8]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 100 100] >> endobj\n".to_vec();
    for number in 2..count + 2 {
        pdf.extend_from_slice(format!("{number} 0 obj << /Subtype /Image /Width {width} /Height {height} /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /{filter} /Length {} >>\nstream\n", data.len()).as_bytes());
        pdf.extend_from_slice(data);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }
    pdf
}

/// pixel bytes を圧縮する。
fn compressed(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// 置換の原因を object ごとに返し、strict では同じ report をエラーに残す。
#[test]
fn replacement_reasons_are_reported_and_rejected() {
    let cases = [
        (
            image_pdf(1, 1, 1, "JPXDecode", b"x"),
            ImageReplacementReason::UnsupportedFilter,
        ),
        (
            image_pdf(1, 1, 1, "FlateDecode", b"broken"),
            ImageReplacementReason::DecodeFailed,
        ),
        (
            image_pdf(1, 2001, 2000, "FlateDecode", b"x"),
            ImageReplacementReason::DecodeFailed,
        ),
    ];
    for (pdf, reason) in cases {
        let parsed = parse_pdf_bytes("quality.pdf", &pdf).unwrap();
        assert_eq!(parsed.quality.issues.len(), parsed.image_elements.len());
        assert!(!parsed.quality.issues.is_empty());
        for (index, issue) in parsed.quality.issues.iter().enumerate() {
            assert_eq!(
                *issue,
                QualityIssue::ImageReplaced {
                    object_number: Some(index as u32 + 2),
                    reason
                }
            );
        }
        let options = ParseOptions {
            strict: true,
            ..Default::default()
        };
        match parse_pdf_bytes_with_options("quality.pdf", &pdf, &options) {
            Err(ParseError::QualityRejected { path, report }) => {
                assert_eq!(path.to_str(), Some("quality.pdf"));
                assert_eq!(report, parsed.quality);
            }
            other => panic!("expected quality rejection: {other:?}"),
        }
    }
}

/// strict でも資源超過を品質警告へ格下げしない。
#[test]
fn resource_limit_takes_precedence_over_quality_rejection() {
    let pdf = image_pdf(1, 1, 1, "FlateDecode", &compressed(&[255; 3]));
    let options = ParseOptions {
        strict: true,
        limits: ParseLimits {
            max_decoded_stream_bytes: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_options("limit.pdf", &pdf, &options),
        Err(ParseError::ResourceLimit { .. })
    ));
}

proptest! {
    /// 実際に decode できた白画像やその他の画素色を置換と誤診しない。
    #[test]
    fn decoded_pixels_never_report_replacement(red in any::<u8>(), green in any::<u8>(), blue in any::<u8>()) {
        let pdf = image_pdf(1, 1, 1, "FlateDecode", &compressed(&[red, green, blue]));
        let parsed = parse_pdf_bytes_with_options("pixels.pdf", &pdf, &ParseOptions { strict: true, ..Default::default() }).unwrap();
        prop_assert_eq!(parsed.image_elements.len(), 1);
        prop_assert!(parsed.quality.issues.is_empty());
    }
}

/// 従来閾値の前後と64枚でも、正常な画像はstrictで置換しない。
#[test]
fn many_decodable_white_images_pass_strict() {
    for count in [20, 21, 64] {
        let pdf = image_pdf(count, 1, 1, "FlateDecode", &compressed(&[255; 3]));
        let parsed = parse_pdf_bytes_with_options(
            "white.pdf",
            &pdf,
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.image_elements.len(), count as usize);
        assert!(parsed.quality.issues.is_empty());
    }
}

/// 圧縮 stream 本文末尾の CR/LF を区切り文字と取り違えない。
#[test]
fn compressed_pixels_ending_in_line_break_keep_declared_length() {
    let data = compressed(&[224, 125, 172]);
    assert!(matches!(data.last(), Some(b'\r') | Some(b'\n')));
    let pdf = image_pdf(1, 1, 1, "FlateDecode", &data);
    let parsed = parse_pdf_bytes_with_options(
        "tail.pdf",
        &pdf,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(parsed.quality.issues.is_empty());
}

/// 未対応 filter chain や metadata に紛れた decoder 名を成功として扱わない。
#[test]
fn unsupported_filter_chain_and_fake_decoder_name_are_reported() {
    let data = compressed(&[255; 3]);
    for filter in [
        "[ /FlateDecode /UnknownDecode ]",
        "/UnknownDecode /Note (FlateDecode)",
    ] {
        let pdf = image_pdf(1, 1, 1, "FlateDecode", &data);
        let pdf = replace_bytes(
            &pdf,
            b"/Filter /FlateDecode",
            format!("/Filter {filter}").as_bytes(),
        );
        let parsed = parse_pdf_bytes("filter.pdf", &pdf).unwrap();
        assert_eq!(
            parsed.quality.issues,
            vec![QualityIssue::ImageReplaced {
                object_number: Some(2),
                reason: ImageReplacementReason::UnsupportedFilter
            }]
        );
    }
}

/// 先頭の指定 byte 列を置換する。
fn replace_bytes(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let offset = bytes.windows(from.len()).position(|w| w == from).unwrap();
    [&bytes[..offset], to, &bytes[offset + from.len()..]].concat()
}

/// 単一要素の filter array と PDF name escape を通常の filter と同じように読む。
#[test]
fn single_filter_array_and_escaped_name_decode_without_warning() {
    for filter in ["[ /FlateDecode ]", "/Flate#44ecode"] {
        let pdf = image_pdf(1, 1, 1, "FlateDecode", &compressed(&[255; 3]));
        let pdf = replace_bytes(
            &pdf,
            b"/Filter /FlateDecode",
            format!("/Filter {filter}").as_bytes(),
        );
        let parsed = parse_pdf_bytes_with_options(
            "escaped.pdf",
            &pdf,
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.image_elements.len(), 1);
        assert!(parsed.quality.issues.is_empty());
    }
}

/// 他の key の name 値を filter/length key と取り違えない。
#[test]
fn dictionary_name_values_do_not_shadow_filter_or_length() {
    let pdf = image_pdf(1, 1, 1, "FlateDecode", &compressed(&[224, 125, 172]));
    let pdf = replace_bytes(
        &pdf,
        b"/BitsPerComponent",
        b"/Note /Filter /Other /Length /BitsPerComponent",
    );
    let parsed = parse_pdf_bytes_with_options(
        "names.pdf",
        &pdf,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(parsed.quality.issues.is_empty());
}

/// 同じ image object を複数回描画しても一度だけ報告し、未使用画像は報告しない。
#[test]
fn report_counts_replaced_objects_not_placements_or_unused_resources() {
    let content = b"q 10 0 0 10 0 0 cm /Im1 Do Q q 10 0 0 10 20 0 cm /Im1 Do Q";
    let mut pdf = image_pdf(2, 1, 1, "JPXDecode", b"x");
    pdf = replace_bytes(
        &pdf,
        b"/Type /Page",
        b"/Type /Page /Resources << /XObject << /Im1 2 0 R /Unused 3 0 R >> >> /Contents 4 0 R",
    );
    pdf.extend_from_slice(format!("4 0 obj << /Length {} >>\nstream\n", content.len()).as_bytes());
    pdf.extend_from_slice(content);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let parsed = parse_pdf_bytes("placements.pdf", &pdf).unwrap();
    assert_eq!(parsed.image_elements.len(), 2);
    assert_eq!(
        parsed.quality.issues,
        vec![
            QualityIssue::ImageReplaced {
                object_number: Some(2),
                reason: ImageReplacementReason::UnsupportedFilter
            },
            QualityIssue::OcrRequired { page: 0 },
        ]
    );
}
