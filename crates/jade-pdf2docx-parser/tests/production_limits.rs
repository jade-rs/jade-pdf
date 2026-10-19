use jade_pdf2docx_parser::{
    ParseError, ParseLimits, parse_pdf, parse_pdf_bytes, parse_pdf_bytes_with_limits,
    parse_pdf_with_limits,
};
use proptest::prelude::*;
use std::io::Write;

/// 小さい圧縮データから任意の長さに展開する本文 stream を作る。
fn compressed_pdf(expanded_len: usize) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&vec![b' '; expanded_len]).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut pdf = format!("%PDF-1.7\n1 0 obj\n<< /Type /Page /Contents 2 0 R >>\nendobj\n2 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n", compressed.len()).into_bytes();
    pdf.extend_from_slice(&compressed);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    pdf
}

/// Filter のない Contents 本文を指定した長さで作る。
fn plain_pdf(length: usize) -> Vec<u8> {
    let mut pdf = format!("%PDF-1.7\n1 0 obj\n<< /Type /Page /Contents 2 0 R >>\nendobj\n2 0 obj\n<< /Length {length} >>\nstream\n").into_bytes();
    pdf.resize(pdf.len() + length, b' ');
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    pdf
}

/// 未圧縮本文もコピー前に個別上限を検査し、境界は受け入れる。
#[test]
fn plain_content_stream_limit_is_inclusive() {
    let limits = ParseLimits {
        max_decoded_stream_bytes: 1024,
        ..Default::default()
    };
    assert!(parse_pdf_bytes_with_limits("exact-plain.pdf", &plain_pdf(1024), &limits).is_ok());
    assert!(matches!(
        parse_pdf_bytes_with_limits("large-plain.pdf", &plain_pdf(1025), &limits),
        Err(ParseError::ResourceLimit {
            resource: "decoded stream bytes",
            limit: 1024,
            actual: 1025,
        })
    ));
}

/// file API でも未圧縮本文の超過を呼び出し元へ返す。
#[test]
fn file_api_applies_plain_content_stream_limit() {
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-plain-stream-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, plain_pdf(1025)).unwrap();
    let limits = ParseLimits {
        max_decoded_stream_bytes: 1024,
        ..Default::default()
    };
    let result = parse_pdf_with_limits(&path, &limits);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        result,
        Err(ParseError::ResourceLimit {
            resource: "decoded stream bytes",
            limit: 1024,
            actual: 1025,
        })
    ));
}

/// 個別上限内の未圧縮本文も、再コピーを累積予算へ数える。
#[test]
fn repeated_plain_content_stream_counts_toward_total_limit() {
    let limits = ParseLimits {
        max_decoded_stream_bytes: 1024,
        max_total_decoded_bytes: 1024,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("total-plain.pdf", &plain_pdf(1024), &limits),
        Err(ParseError::ResourceLimit {
            resource: "total decoded bytes",
            ..
        })
    ));
}

/// 圧縮率によらず stream 展開上限を inclusive に適用する。
#[test]
fn decoded_stream_limit_is_inclusive() {
    let limits = ParseLimits {
        max_decoded_stream_bytes: 1024,
        ..ParseLimits::default()
    };
    assert!(parse_pdf_bytes_with_limits("exact.pdf", &compressed_pdf(1024), &limits).is_ok());
    let result = parse_pdf_bytes_with_limits("bomb.pdf", &compressed_pdf(1025), &limits);
    assert!(matches!(
        result,
        Err(ParseError::ResourceLimit {
            resource: "decoded stream bytes",
            limit: 1024,
            actual: 1025
        })
    ));
}

/// file API の stream 展開も呼び出し元の小さい上限で拒否する。
#[test]
fn file_api_applies_decoded_stream_limit() {
    let path =
        std::env::temp_dir().join(format!("jade-pdf2docx-stream-{}.pdf", std::process::id()));
    std::fs::write(&path, compressed_pdf(1025)).unwrap();
    let limits = ParseLimits {
        max_decoded_stream_bytes: 1024,
        ..ParseLimits::default()
    };
    let result = parse_pdf_with_limits(&path, &limits);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        result,
        Err(ParseError::ResourceLimit {
            resource: "decoded stream bytes",
            ..
        })
    ));
}

/// 個別 stream が上限以下でも繰り返し展開の累積予算を超えたら失敗する。
#[test]
fn repeated_stream_decoding_counts_toward_total_limit() {
    let limits = ParseLimits {
        max_decoded_stream_bytes: 1024,
        max_total_decoded_bytes: 1024,
        ..ParseLimits::default()
    };
    let result = parse_pdf_bytes_with_limits("total.pdf", &compressed_pdf(1024), &limits);
    assert!(matches!(
        result,
        Err(ParseError::ResourceLimit {
            resource: "total decoded bytes",
            ..
        })
    ));
}

/// 画像 decode の Option 失敗を白画像で置き換えても資源超過を隠さない。
#[test]
fn image_stream_limit_is_not_hidden_by_placeholder_fallback() {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&vec![0; 1025]).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut pdf = format!("%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n2 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 3 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode /Length {} >>\nstream\n", compressed.len()).into_bytes();
    pdf.extend_from_slice(&compressed);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let limits = ParseLimits {
        max_decoded_stream_bytes: 1024,
        ..Default::default()
    };
    let result = parse_pdf_bytes_with_limits("image-bomb.pdf", &pdf, &limits);
    assert!(matches!(
        result,
        Err(ParseError::ResourceLimit {
            resource: "decoded stream bytes",
            ..
        })
    ));
}

/// PNG保存の不足はfile/bytes APIで同じ型付きエラーとなり、次の解析へ持ち越さない。
#[test]
fn image_png_storage_limit_is_public_and_local_to_parse() {
    let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n2 0 obj\n<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n";
    let limits = ParseLimits {
        max_total_image_storage_bytes: 0,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("png-storage.pdf", pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            limit: 0,
            ..
        })
    ));
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-png-storage-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, pdf).unwrap();
    assert!(matches!(
        {
            let result = parse_pdf_with_limits(&path, &limits);
            std::fs::remove_file(&path).unwrap();
            result
        },
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            limit: 0,
            ..
        })
    ));
    assert_eq!(
        parse_pdf_bytes("png-storage.pdf", pdf)
            .unwrap()
            .image_elements
            .len(),
        1
    );
}

/// 資源超過した解析の停止状態を次の独立した解析へ持ち越さない。
#[test]
fn decode_budget_is_local_to_each_parse() {
    let limits = ParseLimits {
        max_decoded_stream_bytes: 1024,
        ..Default::default()
    };
    assert!(parse_pdf_bytes_with_limits("first.pdf", &compressed_pdf(1025), &limits).is_err());
    assert!(parse_pdf_bytes_with_limits("second.pdf", &compressed_pdf(1024), &limits).is_ok());
}

/// PDF 辞書では 1x1 と偽った画像を、内部寸法の 2x3 として制限する。
fn assert_codec_dimensions_are_bounded(filter: &str, data: &[u8]) {
    let mut pdf = format!("%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n2 0 obj\n<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /{filter} /Length {} >>\nstream\n", data.len()).into_bytes();
    pdf.extend_from_slice(data);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let limits = ParseLimits {
        max_image_pixels: 5,
        ..Default::default()
    };
    let result = parse_pdf_bytes_with_limits("codec-dimensions.pdf", &pdf, &limits);
    assert!(
        matches!(
            result,
            Err(ParseError::ResourceLimit {
                resource: "image pixels",
                limit: 5,
                actual: 6
            })
        ),
        "codec dimensions were ignored: {result:?}"
    );
}

/// JPEG は header を読んで画素数を確認してから pixel decode を開始する。
#[test]
fn jpeg_dimensions_cannot_bypass_pdf_dictionary_limit() {
    // SOI + 2x3 grayscale SOF + SOS。画素上限判定には entropy data は不要。
    let jpeg = [
        0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 3, 0, 2, 1, 1, 0x11, 0, 0xff, 0xda, 0, 8, 1, 1, 0, 0,
        63, 0,
    ];
    assert_codec_dimensions_are_bounded("DCTDecode", &jpeg);
}

/// JBIG2 の page information 寸法を RGB buffer 確保前に確認する。
#[test]
fn jbig2_dimensions_cannot_bypass_pdf_dictionary_limit() {
    // Segment number 1 / PageInformation / page 1 / payload 19 bytes。
    let mut jbig2 = vec![0, 0, 0, 1, 48, 0, 1, 0, 0, 0, 19];
    jbig2.extend_from_slice(&2u32.to_be_bytes());
    jbig2.extend_from_slice(&3u32.to_be_bytes());
    jbig2.extend_from_slice(&[0; 11]);
    assert_codec_dimensions_are_bounded("JBIG2Decode", &jbig2);
}

/// CCITT の実際の寸法を辞書の Width/Height と独立に、境界を含めて制限する。
#[test]
fn ccitt_decode_parameters_are_bounded_before_allocation() {
    for (width, height, parameters, pixels) in [
        (1, 1, "/Columns 3 /Rows 2 /EndOfBlock false", 6),
        (1, 2, "/Columns 3", 6),
        (3, 1, "/Rows 2 /EndOfBlock false", 3456),
        (1, 2, "/Columns 3 /Rows 0", 6),
        (1, 2, "/Columns 3 /Rows 65536", 6),
    ] {
        let pdf = format!(
            "%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n2 0 obj << /Subtype /Image /Width {width} /Height {height} /Filter /CCITTFaxDecode /DecodeParms << {parameters} /K -1 >> /Length 1 >>\nstream\nx\nendstream\nendobj\n"
        );
        let limits = ParseLimits {
            max_image_pixels: pixels,
            ..Default::default()
        };
        assert!(parse_pdf_bytes_with_limits("ccitt.pdf", pdf.as_bytes(), &limits).is_ok());
        let limits = ParseLimits {
            max_image_pixels: pixels - 1,
            ..Default::default()
        };
        assert!(
            matches!(
                parse_pdf_bytes_with_limits("ccitt.pdf", pdf.as_bytes(), &limits),
                Err(ParseError::ResourceLimit {
                    resource: "image pixels",
                    actual,
                    limit,
                }) if actual == pixels && limit == pixels - 1
            ),
            "CCITT DecodeParms bypassed the image pixel limit"
        );
    }
}

proptest! {
    /// CCITT の buffer 寸法が PDF 辞書より大きくても同じ画素上限を適用する。
    #[test]
    fn ccitt_pixel_limit_matches_decode_parameters(columns in 1u32..33, rows in 1u32..33) {
        let pdf = format!("%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n2 0 obj << /Subtype /Image /Width 1 /Height 1 /Filter /CCITTFaxDecode /DecodeParms << /Columns {columns} /Rows {rows} /K -1 /EndOfBlock false >> /Length 1 >>\nstream\nx\nendstream\nendobj\n");
        let pixels = u64::from(columns) * u64::from(rows);
        let limits = ParseLimits { max_image_pixels: pixels, ..Default::default() };
        prop_assert!(parse_pdf_bytes_with_limits("ccitt-property.pdf", pdf.as_bytes(), &limits).is_ok());
        let limits = ParseLimits { max_image_pixels: pixels - 1, ..Default::default() };
        prop_assert!(matches!(parse_pdf_bytes_with_limits("ccitt-property.pdf", pdf.as_bytes(), &limits), Err(ParseError::ResourceLimit { resource: "image pixels", .. })), "CCITT pixels were not bounded");
    }

    /// 寸法の積に対して境界を含める規則を小さい画像で検証する。
    #[test]
    fn image_pixel_limit_matches_dimension_product(width in 1u32..17, height in 1u32..17) {
        let pdf = format!("%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n2 0 obj\n<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /Filter /Unsupported /Length 1 >>\nstream\nx\nendstream\nendobj\n");
        let pixels = u64::from(width) * u64::from(height);
        let mut limits = ParseLimits { max_image_pixels: pixels, ..ParseLimits::default() };
        prop_assert!(parse_pdf_bytes_with_limits("property.pdf", pdf.as_bytes(), &limits).is_ok());
        limits.max_image_pixels -= 1;
        let result = parse_pdf_bytes_with_limits("property.pdf", pdf.as_bytes(), &limits);
        prop_assert!(matches!(result, Err(ParseError::ResourceLimit { resource: "image pixels", .. })), "image limit was not enforced");
    }
}

/// ちょうど上限の入力は許容し、1 byte 超過を型付きエラーで返す。
#[test]
fn byte_limit_is_inclusive_and_reported() {
    let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n";
    let mut limits = ParseLimits {
        max_input_bytes: pdf.len() as u64,
        ..ParseLimits::default()
    };
    assert!(parse_pdf_bytes_with_limits("bounded.pdf", pdf, &limits).is_ok());
    limits.max_input_bytes -= 1;
    assert!(matches!(
        parse_pdf_bytes_with_limits("bounded.pdf", pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "input bytes",
            ..
        })
    ));
}

/// ファイル API も読み込み時に同じ入力上限を適用する。
#[test]
fn file_api_applies_input_limit() {
    let path = std::env::temp_dir().join(format!("jade-pdf2docx-limit-{}.pdf", std::process::id()));
    std::fs::write(&path, b"%PDF-1.7\n").unwrap();
    let limits = ParseLimits {
        max_input_bytes: 8,
        ..ParseLimits::default()
    };
    let result = parse_pdf_with_limits(&path, &limits);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        result,
        Err(ParseError::ResourceLimit {
            resource: "input bytes",
            ..
        })
    ));
}

/// ページ数・画素数の上限を呼び出し元が小さく設定できる。
#[test]
fn configurable_page_and_image_limits() {
    let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n2 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 3 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /Unsupported /Length 1 >>\nstream\nx\nendstream\nendobj\n";
    let mut limits = ParseLimits {
        max_pages: 1,
        max_image_pixels: 6,
        ..ParseLimits::default()
    };
    assert!(parse_pdf_bytes_with_limits("bounded.pdf", pdf, &limits).is_ok());
    limits.max_image_pixels = 5;
    assert!(matches!(
        parse_pdf_bytes_with_limits("bounded.pdf", pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "image pixels",
            ..
        })
    ));
    limits.max_image_pixels = 6;
    limits.max_pages = 0;
    assert!(matches!(
        parse_pdf_bytes_with_limits("bounded.pdf", pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "page count",
            ..
        })
    ));
}

/// 既定の file API もページ上限を適用する。
#[test]
fn file_api_applies_default_page_limit() {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    for number in 1..=1001 {
        pdf.extend_from_slice(format!("{number} 0 obj\n<< /Type /Page >>\nendobj\n").as_bytes());
    }
    let path = std::env::temp_dir().join(format!("jade-pdf2docx-pages-{}.pdf", std::process::id()));
    std::fs::write(&path, pdf).unwrap();
    let result = parse_pdf(&path);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        result,
        Err(ParseError::ResourceLimit {
            resource: "page count",
            ..
        })
    ));
}

/// 多数のページを持つ PDF は本文を展開する前に上限超過として拒否する。
#[test]
fn excessive_page_count_is_rejected() {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    for number in 1..=1001 {
        pdf.extend_from_slice(format!("{number} 0 obj\n<< /Type /Page >>\nendobj\n").as_bytes());
    }
    let result = parse_pdf_bytes("too-many-pages.pdf", &pdf);
    assert!(result.is_err(), "1001 pages must exceed the default limit");
    assert!(result.unwrap_err().to_string().contains("page count"));
}

/// huge な画像の placeholder を作る前に画素数超過として拒否する。
#[test]
fn excessive_image_dimensions_are_rejected() {
    let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n2 0 obj\n<< /Type /XObject /Subtype /Image /Width 100000 /Height 100000 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /Unsupported /Length 1 >>\nstream\nx\nendstream\nendobj\n";
    let result = parse_pdf_bytes("huge-image.pdf", pdf);
    assert!(
        result.is_err(),
        "image must be rejected before fallback allocation"
    );
    assert!(result.unwrap_err().to_string().contains("image pixels"));
}
