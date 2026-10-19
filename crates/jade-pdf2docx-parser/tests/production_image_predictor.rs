//! Predictor復元と失敗時の品質診断を公開APIで保証する。
use jade_pdf2docx_parser::{
    ImageReplacementReason, ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use std::io::{Read, Write};

/// Predictor画像と任意の補助objectを持つ正式xref付きPDFを作る。
fn pdf(parameters: &str, encoded: &[u8], extra: &[&[u8]]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(encoded).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut image = format!("<< /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode /DecodeParms {parameters} /Length {} >>\nstream\n", compressed.len()).into_bytes();
    image.extend_from_slice(&compressed);
    image.extend_from_slice(b"\nendstream");
    let content = b"q 20 0 0 10 0 0 cm /Im Do Q BT (retained text) Tj ET";
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im 4 0 R >> >> /Contents {} 0 R >>", 5+extra.len()).into_bytes(),
        image,
    ];
    objects.extend(extra.iter().map(|bytes| bytes.to_vec()));
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content);
    stream.extend_from_slice(b"\nendstream");
    objects.push(stream);
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

/// 小さなPNGのscanlineを上限付きで復号する。
fn scanline(png: &[u8]) -> Vec<u8> {
    let mut idat = Vec::new();
    let mut cursor = 8;
    while cursor < png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
        }
        cursor += length + 12;
    }
    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .take(64)
        .read_to_end(&mut decoded)
        .unwrap();
    decoded
}

/// 正常Predictorは画素と本文を保持し、画像本文の確認要求だけを残す。
#[test]
fn valid_predictors_keep_pixels() {
    for (parameters, encoded) in [
        (
            "<< /Predictor 11 /Colors 3 /Columns 2 >>",
            &[1, 10, 20, 30, 30, 30, 30][..],
        ),
        (
            "<< /Predictor 2 /Colors 3 /Columns 2 >>",
            &[10, 20, 30, 30, 30, 30][..],
        ),
    ] {
        let parsed = parse_pdf_bytes_with_options(
            "predictor.pdf",
            &pdf(parameters, encoded, &[]),
            &ParseOptions {
                strict: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            parsed.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        assert_eq!(
            scanline(&parsed.image_elements[0].png_bytes),
            [0, 10, 20, 30, 40, 50, 60]
        );
        assert!(parsed.text_preview.contains("retained text"));
    }
}

/// 間接辞書・間接scalar・escape名と単一配列を解決する。
#[test]
fn indirect_predictor_parameters_keep_pixels() {
    let extra: &[&[u8]] = &[
        b"<< /Pre#64ictor 6 0 R /Colors 7 0 R /Columns 8 0 R /BitsPerComponent 9 0 R /Note << /Predictor 0 >> >>",
        b"+11", b"+3", b"+2", b"+8",
    ];
    for parameters in ["5 0 R", "[5 0 R]"] {
        let parsed = parse_pdf_bytes_with_options(
            "indirect.pdf",
            &pdf(parameters, &[1, 10, 20, 30, 30, 30, 30], extra),
            &ParseOptions {
                strict: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            scanline(&parsed.image_elements[0].png_bytes),
            [0, 10, 20, 30, 40, 50, 60]
        );
    }
}

/// 不正Predictorは通常reportで露出し、strictでは同じreportで拒否する。
#[test]
fn invalid_predictors_are_reported_and_rejected() {
    for parameters in [
        "<< /Predictor 3 >>",
        "<< /Predictor 12 /Columns 0 >>",
        "<< /Predictor 12 /Colors false >>",
        "<< /Predictor 12 /Colors 3 /Columns 2 >>",
    ] {
        let bytes = pdf(parameters, &[10, 20, 30, 40, 50, 60], &[]);
        let parsed =
            parse_pdf_bytes_with_options("invalid.pdf", &bytes, &ParseOptions::default()).unwrap();
        assert_eq!(
            parsed.quality.issues,
            [
                QualityIssue::ImageReplaced {
                    object_number: Some(4),
                    reason: ImageReplacementReason::DecodeFailed
                },
                QualityIssue::OcrRequired { page: 0 }
            ]
        );
        match parse_pdf_bytes_with_options(
            "invalid.pdf",
            &bytes,
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
        ) {
            Err(ParseError::QualityRejected { report, .. }) => assert_eq!(report, parsed.quality),
            other => panic!("expected strict rejection: {other:?}"),
        }
    }
}
