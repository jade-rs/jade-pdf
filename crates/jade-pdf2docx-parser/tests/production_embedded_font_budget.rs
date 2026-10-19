//! 埋め込み Font の保存・修復予算を公開 API で検査する回帰。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes_with_limits};

use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

/// OS/2 の installable embedding を明示した小さい sfnt payload を作る。
fn allowed_font(length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length.max(38)];
    bytes[..4].copy_from_slice(&[0, 1, 0, 0]);
    bytes[4..6].copy_from_slice(&1u16.to_be_bytes());
    bytes[12..16].copy_from_slice(b"OS/2");
    bytes[20..24].copy_from_slice(&28u32.to_be_bytes());
    bytes[24..28].copy_from_slice(&10u32.to_be_bytes());
    bytes
}

/// 公開入口のmetadata境界でもFont名・候補・出力を保存し、超過は型付きエラーにする。
#[test]
fn embedded_font_metadata_boundary_is_inclusive_and_independent() {
    let font = allowed_font(38);
    let bytes = font_pdf(&[
        ("FontB", &font, false),
        ("#46ontA", &font, false),
        ("FontA", &font, false),
    ]);
    let mut rejected = 0;
    let mut capacity = ParseLimits::default().max_total_metadata_workspace_bytes;
    // 既定容量で成功することを確認し、最大27回の二分探索で最小成功容量を求める。
    let expected =
        parse_pdf_bytes_with_limits("default.pdf", &bytes, &ParseLimits::default()).unwrap();
    while capacity - rejected > 1 {
        let middle = rejected + (capacity - rejected) / 2;
        match parse_pdf_bytes_with_limits(
            "metadata.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: middle,
                ..Default::default()
            },
        ) {
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) if actual > middle => rejected = middle,
            Ok(parsed) => {
                assert_eq!(parsed.text_preview, expected.text_preview);
                assert_eq!(
                    parsed
                        .embedded_fonts
                        .iter()
                        .map(|font| font.font_name.as_str())
                        .collect::<Vec<_>>(),
                    ["FontA", "FontB"]
                );
                assert!(
                    parsed
                        .embedded_fonts
                        .iter()
                        .all(|saved| saved.bytes == font)
                );
                capacity = middle;
            }
            other => panic!("unexpected metadata boundary: {other:?}"),
        }
    }
    assert!(capacity > 0);
    assert!(matches!(
        parse_pdf_bytes_with_limits(
            "short.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity - 1,
                ..Default::default()
            }
        ),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    let parsed = parse_pdf_bytes_with_limits(
        "again.pdf",
        &bytes,
        &ParseLimits {
            max_total_metadata_workspace_bytes: capacity,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.embedded_fonts.len(), 2);
}

/// 正確な Length と xref を持ち、任意の binary Font stream を保存する PDF を作る。
fn font_pdf(fonts: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F 5 0 R >> >> >>".to_vec(),
        Vec::new(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    // 本文の Length は余分な改行を含めず実際の byte 数に一致させる。
    let content = b"BT /F 12 Tf 10 10 Td (A) Tj ET";
    objects[3] = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    objects[3].extend_from_slice(content);
    objects[3].extend_from_slice(b"\nendstream");
    for &(name, font, compressed) in fonts {
        let file_number = objects.len() + 2;
        objects.push(
            format!("<< /Type /FontDescriptor /FontName /{name} /FontFile2 {file_number} 0 R >>")
                .into_bytes(),
        );
        let payload = if compressed {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(font).unwrap();
            encoder.finish().unwrap()
        } else {
            font.to_vec()
        };
        let filter = if compressed {
            "/Filter /FlateDecode"
        } else {
            ""
        };
        let mut stream = format!("<< /Length {} {filter} >>\nstream\n", payload.len()).into_bytes();
        stream.extend_from_slice(&payload);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
    }
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// 二つの別名 Font の保存は累積し、同名候補は byte の読込前に除く。
#[test]
fn raw_font_capacity_counts_candidates_and_skips_same_name() {
    let font = allowed_font(38);
    let bytes = font_pdf(&[
        ("FontA", &font, false),
        ("FontA", &font, false),
        ("FontB", &font, false),
    ]);
    let capacity = (2 * font.len()) as u64;
    let parsed = parse_pdf_bytes_with_limits(
        "raw.pdf",
        &bytes,
        &ParseLimits {
            max_total_embedded_font_bytes: capacity,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.embedded_fonts.len(), 2);
    assert!(
        parsed
            .embedded_fonts
            .iter()
            .all(|embedded| embedded.bytes == font)
    );
    assert!(
        matches!(parse_pdf_bytes_with_limits("raw.pdf", &bytes, &ParseLimits {
        max_total_embedded_font_bytes: capacity - 1, ..Default::default()
    }), Err(ParseError::ResourceLimit { resource: "embedded font bytes", actual, .. }) if actual == capacity)
    );
}

/// 圧縮 Font は保存配列の拡大も数え、上限ちょうどなら payload を変えず成功する。
#[test]
fn compressed_font_capacity_is_inclusive_and_per_parse() {
    let font = allowed_font(20000);
    let bytes = font_pdf(&[("FontA", &font, true)]);
    let capacity = 8192 + 16384 + 32768;
    for _ in 0..2 {
        let parsed = parse_pdf_bytes_with_limits(
            "compressed.pdf",
            &bytes,
            &ParseLimits {
                max_total_embedded_font_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.embedded_fonts.len(), 1);
        assert_eq!(parsed.embedded_fonts[0].bytes, font);
        assert!(
            matches!(parse_pdf_bytes_with_limits("compressed.pdf", &bytes, &ParseLimits {
            max_total_embedded_font_bytes: capacity - 1, ..Default::default()
        }), Err(ParseError::ResourceLimit { resource: "embedded font bytes", actual, .. }) if actual == capacity)
        );
    }
}

/// 失敗する候補の buffer も解放後に予算へ残し、後続の保存を制限する。
#[test]
fn discarded_font_payload_still_consumes_budget() {
    let invalid = vec![b'x'; 38];
    let valid = allowed_font(38);
    let bytes = font_pdf(&[("Invalid", &invalid, false), ("Valid", &valid, false)]);
    assert!(matches!(
        parse_pdf_bytes_with_limits(
            "discarded.pdf",
            &bytes,
            &ParseLimits {
                max_total_embedded_font_bytes: 75,
                ..Default::default()
            }
        ),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            actual: 76,
            ..
        })
    ));
    let parsed = parse_pdf_bytes_with_limits(
        "discarded.pdf",
        &bytes,
        &ParseLimits {
            max_total_embedded_font_bytes: 76,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.embedded_fonts.len(), 1);
    assert_eq!(parsed.embedded_fonts[0].bytes, valid);
}
