//! 直接・間接 Length と本文内 marker を本文抽出でも保持する回帰。
use std::io::Write;

use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;

/// generation と body を保ち、Catalog・Pages と画像の明示的な描画を補う。
fn pdf(objects: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut objects = objects.to_vec();
    let root = if objects[0].1.starts_with(b"<< /Type /Catalog") {
        1
    } else {
        let catalog = objects.len() + 1;
        let pages = catalog + 1;
        let image = objects[1].1.starts_with(b"<< /Subtype /Image");
        let mut page = format!("<< /Parent {pages} 0 R ").into_bytes();
        if !objects[0].1.windows(9).any(|key| key == b"/MediaBox") {
            page.extend_from_slice(b"/MediaBox [0 0 100 100] ");
        }
        if image {
            page.extend_from_slice(
                format!(
                    "/Resources << /XObject << /Im 2 0 R >> >> /Contents {} 0 R ",
                    pages + 1
                )
                .as_bytes(),
            );
        }
        page.extend_from_slice(&objects[0].1[2..]);
        objects[0].1 = page;
        objects.push((
            0,
            format!("<< /Type /Catalog /Pages {pages} 0 R >>").into_bytes(),
        ));
        objects.push((0, b"<< /Type /Pages /Count 1 /Kids [1 0 R] >>".to_vec()));
        if image {
            let content = b"BT (x) Tj ET q 1 0 0 1 0 0 cm /Im Do Q";
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(content);
            stream.extend_from_slice(b"\nendstream");
            objects.push((0, stream));
        }
        catalog
    };
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, (generation, body)) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} {generation} obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for (offset, (generation, _)) in offsets.iter().zip(&objects) {
        bytes.extend_from_slice(format!("{offset:010} {generation:05} n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer << /Size {} /Root {root} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// 圧縮本文にも marker が実 byte として現れるよう zlib stored block を使う。
fn stored(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// 本文の直接・間接 Length と圧縮の組合せを生成する。
fn content_pdf(text: &str, indirect: bool, flate: bool) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 10 20 Td ({text}) Tj ET").into_bytes();
    let data = if flate { stored(&content) } else { content };
    let length = if indirect {
        "4 7 R".to_owned()
    } else {
        data.len().to_string()
    };
    let filter = if flate { "/Filter /FlateDecode" } else { "" };
    let mut body = format!("<< /Length {length} {filter} >>\nstream\n").into_bytes();
    body.extend_from_slice(&data);
    body.extend_from_slice(b"\nendstream");
    pdf(&[
        (0, b"<< /Type /Catalog /Pages 6 0 R >>".to_vec()),
        (0, b"<< /Type /Page /Parent 6 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 3 0 R >>".to_vec()),
        (0, body),
        (7, data.len().to_string().into_bytes()),
        (0, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
        (0, b"<< /Type /Pages /Count 1 /Kids [2 0 R] >>".to_vec()),
    ])
}

/// 本文中の marker は抽出終了にならず、前後を含む全文を保持する。
#[test]
fn content_marker_preserves_positioned_text_and_preview() {
    let text = "before endstream after";
    for indirect in [false, true] {
        for flate in [false, true] {
            let parsed =
                parse_pdf_bytes("content.pdf", &content_pdf(text, indirect, flate)).unwrap();
            assert_eq!(
                parsed.text_elements.len(),
                1,
                "indirect={indirect}, flate={flate}"
            );
            assert_eq!(parsed.text_elements[0].text, text);
            assert_eq!(parsed.text_preview, text);
        }
    }
}

proptest! {
    /// 任意の安全な ASCII 前後文字を含む stream でも本文を失わない。
    #[test]
    fn indirect_content_preserves_marker_surroundings(prefix in "[a-z]{0,24}", suffix in "[a-z]{0,24}", flate in any::<bool>()) {
        let text = format!("{prefix}endstream{suffix}");
        let parsed = parse_pdf_bytes("content.pdf", &content_pdf(&text, true, flate)).unwrap();
        prop_assert_eq!(parsed.text_elements.len(), 1);
        prop_assert_eq!(&parsed.text_elements[0].text, &text);
        prop_assert_eq!(parsed.text_preview, text);
    }
}

/// 任意辞書と間接 Length を持つ補助 stream を作る。
fn auxiliary_stream(data: &[u8], dictionary: &str, flate: bool) -> Vec<u8> {
    let filter = if flate { "/Filter /FlateDecode" } else { "" };
    let mut body = format!("<< /Length 4 7 R {dictionary} {filter} >>\nstream\n").into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// marker より後の ToUnicode 対応を座標付き本文と preview に適用する。
#[test]
fn tounicode_marker_preserves_mapping() {
    let cmap = b"% endstream\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <3042>\n<42> <3044>\nendbfchar\n";
    for flate in [false, true] {
        let data = if flate { stored(cmap) } else { cmap.to_vec() };
        let parsed = parse_pdf_bytes("cmap.pdf", &pdf(&[
            (0, b"<< /Type /Page /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 2 0 R >>".to_vec()),
            (0, b"<< /Length 34 >>\nstream\nBT /F1 12 Tf 10 20 Td <4142> Tj ET\nendstream".to_vec()),
            (0, auxiliary_stream(&data, "", flate)),
            (7, data.len().to_string().into_bytes()),
            (0, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 3 0 R >>".to_vec()),
        ])).unwrap();
        assert_eq!(parsed.text_elements[0].text, "あい", "flate={flate}");
        assert_eq!(parsed.text_preview, "あい");
    }
}

/// 埋め込み可能な最小 sfnt 風 fixture の末尾 marker と改行を byte 単位で保持する。
#[test]
fn embedded_font_preserves_marker_and_trailing_bytes() {
    let mut font = vec![0u8; 38];
    font[..4].copy_from_slice(b"\0\x01\0\0");
    font[4..6].copy_from_slice(&1u16.to_be_bytes());
    font[12..16].copy_from_slice(b"OS/2");
    font[20..24].copy_from_slice(&28u32.to_be_bytes());
    font[24..28].copy_from_slice(&10u32.to_be_bytes());
    font.extend_from_slice(b"endstream\r\n");
    for flate in [false, true] {
        let data = if flate { stored(&font) } else { font.clone() };
        let parsed = parse_pdf_bytes(
            "font.pdf",
            &pdf(&[
                (0, b"<< /Type /Page >>".to_vec()),
                (
                    0,
                    b"<< /Type /FontDescriptor /FontName /BoundaryFont /FontFile2 3 0 R >>"
                        .to_vec(),
                ),
                (0, auxiliary_stream(&data, "", flate)),
                (7, data.len().to_string().into_bytes()),
            ]),
        )
        .unwrap();
        assert_eq!(parsed.embedded_fonts.len(), 1, "flate={flate}");
        assert_eq!(parsed.embedded_fonts[0].bytes, font);
    }
}

/// parser が生成する PNG の IDAT を展開する。
fn png_scanline(png: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut cursor = 8;
    let mut idat = Vec::new();
    while cursor + 12 <= png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
        }
        cursor += length + 12;
    }
    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    decoded
}

/// Indexed palette の marker を色成分として読み、palette 全体を保持する。
#[test]
fn palette_stream_marker_preserves_rgb() {
    let palette = b"endstreamABC";
    for flate in [false, true] {
        let data = if flate {
            stored(palette)
        } else {
            palette.to_vec()
        };
        let image = stored(&[0, 1, 2, 3]);
        let mut body = format!("<< /Subtype /Image /Width 4 /Height 1 /BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 3 3 0 R] /Filter /FlateDecode /Length {} >>\nstream\n", image.len()).into_bytes();
        body.extend_from_slice(&image);
        body.extend_from_slice(b"\nendstream");
        let parsed = parse_pdf_bytes(
            "palette.pdf",
            &pdf(&[
                (0, b"<< /Type /Page >>".to_vec()),
                (0, body),
                (0, auxiliary_stream(&data, "", flate)),
                (7, data.len().to_string().into_bytes()),
            ]),
        )
        .unwrap();
        assert_eq!(
            parsed.quality.issues,
            [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
        );
        let mut expected = vec![0];
        expected.extend_from_slice(palette);
        assert_eq!(
            png_scanline(&parsed.image_elements[0].png_bytes),
            expected,
            "flate={flate}"
        );
    }
}

/// soft mask の圧縮本文中 marker でも alpha を捨てない。
#[test]
fn soft_mask_stream_marker_preserves_alpha() {
    let alpha = b"endstream";
    let data = stored(alpha);
    let rgb = vec![100; alpha.len() * 3];
    let image = stored(&rgb);
    let mut body = format!("<< /Subtype /Image /Width 9 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /SMask 3 0 R /Filter /FlateDecode /Length {} >>\nstream\n", image.len()).into_bytes();
    body.extend_from_slice(&image);
    body.extend_from_slice(b"\nendstream");
    let parsed = parse_pdf_bytes("mask.pdf", &pdf(&[
        (0, b"<< /Type /Page >>".to_vec()),
        (0, body),
        (0, auxiliary_stream(&data, "/Subtype /Image /Width 9 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray", true)),
        (7, data.len().to_string().into_bytes()),
    ])).unwrap();
    let mut expected = vec![0];
    for alpha in alpha {
        expected.extend_from_slice(&[100, 100, 100, *alpha]);
    }
    assert_eq!(png_scanline(&parsed.image_elements[0].png_bytes), expected);
}

/// xref 付き入力は有効な Contents の線分だけを読み、本文内の偽画像を数えない。
#[test]
fn graphics_contents_ignore_unreferenced_streams_and_fake_images() {
    let content = stored(b"0 10 m 100 10 l S");
    let mut body = format!(
        "<< /Filter /FlateDecode /Length {} >>\nstream\n",
        content.len()
    )
    .into_bytes();
    body.extend_from_slice(&content);
    body.extend_from_slice(b"\nendstream");
    let mut fake = b"endstream\nendobj\n".to_vec();
    for number in 10..31 {
        fake.extend_from_slice(format!("{number} 0 obj << /Subtype /Image /Width 1 /Height 1 /Length 3 >> stream\nabc\nendstream\nendobj\n").as_bytes());
    }
    for referenced in [false, true] {
        let page = if referenced {
            b"<< /Type /Page /Contents 2 0 R >>".to_vec()
        } else {
            b"<< /Type /Page >>".to_vec()
        };
        let parsed = parse_pdf_bytes(
            "graphics.pdf",
            &pdf(&[
                (0, page),
                (0, body.clone()),
                (0, auxiliary_stream(&fake, "", false)),
                (7, fake.len().to_string().into_bytes()),
            ]),
        )
        .unwrap();
        assert_eq!(
            parsed.graphics.len(),
            usize::from(referenced),
            "referenced={referenced}"
        );
        assert!(parsed.image_elements.is_empty());
    }
}
