//! 構造検査で解決した stream 境界を画像抽出でも保持する回帰。
use std::io::{Read, Write};

use jade_pdf2docx_parser::{ParseOptions, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// xref が整数 Length object を指す画像 PDF を生成する。
fn image_pdf(data: &[u8], width: usize, indirect: bool) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0, pdf.len()];
    pdf.extend_from_slice(b"1 0 obj << /Type /Page /MediaBox [0 0 100 100] >> endobj\n");
    offsets.push(pdf.len());
    let length = if indirect {
        "3 7 R".to_owned()
    } else {
        data.len().to_string()
    };
    pdf.extend_from_slice(format!("2 0 obj << /Subtype /Image /Width {width} /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode /Length {length} >>\nstream\n").as_bytes());
    pdf.extend_from_slice(data);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    offsets.push(pdf.len());
    pdf.extend_from_slice(format!("3 7 obj {} endobj\n", data.len()).as_bytes());
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for (number, offset) in offsets.iter().enumerate().skip(1) {
        let generation = if number == 3 { 7 } else { 0 };
        pdf.extend_from_slice(format!("{offset:010} {generation:05} n \n").as_bytes());
    }
    pdf.extend_from_slice(format!("trailer << /Size 4 >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    pdf
}

/// zlib stream を指定圧縮率で生成する。
fn compressed(pixels: &[u8], level: flate2::Compression) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), level);
    encoder.write_all(pixels).unwrap();
    encoder.finish().unwrap()
}

/// 生成 PNG の filter 0 の RGB 画素を読み、白 fallback でないことも検証する。
fn pixels_from_pdf(pdf: &[u8]) -> Vec<u8> {
    let parsed = parse_pdf_bytes_with_options(
        "boundaries.pdf",
        pdf,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(parsed.quality.issues.is_empty());
    assert_eq!(parsed.image_elements.len(), 1);
    let png = &parsed.image_elements[0].png_bytes;
    let mut cursor = 8;
    let mut idat = Vec::new();
    while cursor + 12 <= png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
        }
        cursor += length + 12;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    assert_eq!(raw[0], 0);
    raw[1..].to_vec()
}

/// 間接 Length の stream 本文末尾にある改行も画像 byte として保持する。
#[test]
fn indirect_image_length_preserves_trailing_line_break() {
    let pixels = [224, 125, 172];
    let data = compressed(&pixels, flate2::Compression::default());
    assert!(matches!(data.last(), Some(b'\r') | Some(b'\n')));
    assert_eq!(pixels_from_pdf(&image_pdf(&data, 1, true)), pixels);
}

/// 圧縮本文に endstream が現れても、直接・間接 Length の境界まで decode する。
#[test]
fn image_payload_marker_does_not_terminate_stream() {
    let pixels = b"endstreamABC";
    let data = compressed(pixels, flate2::Compression::none());
    assert!(data.windows(9).any(|window| window == b"endstream"));
    for indirect in [false, true] {
        assert_eq!(pixels_from_pdf(&image_pdf(&data, 4, indirect)), pixels);
    }
}

/// binary 本文内の偽画像 object は枚数制限にも抽出結果にも含めない。
#[test]
fn image_payload_fake_objects_do_not_trigger_image_count_limit() {
    let mut pixels = b"endstream\nendobj\n".to_vec();
    for number in 10..31 {
        pixels.extend_from_slice(format!("{number} 0 obj << /Subtype /Image /Width 1 /Height 1 /Length 3 >> stream\nabc\nendstream\nendobj\n").as_bytes());
    }
    while !pixels.len().is_multiple_of(3) {
        pixels.push(0);
    }
    let data = compressed(&pixels, flate2::Compression::none());
    for indirect in [false, true] {
        assert_eq!(
            pixels_from_pdf(&image_pdf(&data, pixels.len() / 3, indirect)),
            pixels
        );
    }
}

/// binary 本文内の偽寸法で実画像の資源上限を誤判定しない。
#[test]
fn image_payload_fake_dimensions_do_not_trigger_pixel_limit() {
    let mut pixels = b"endstream\nendobj\n99 0 obj << /Subtype /Image /Width 4294967295 /Height 4294967295 /Length 3 >> stream\nabc\nendstream\nendobj\n".to_vec();
    while !pixels.len().is_multiple_of(3) {
        pixels.push(0);
    }
    let data = compressed(&pixels, flate2::Compression::none());
    assert_eq!(
        pixels_from_pdf(&image_pdf(&data, pixels.len() / 3, true)),
        pixels
    );
}

proptest! {
    /// xref で解決する長さでも任意の画素と marker 周辺 byte を保持する。
    #[test]
    fn indirect_length_preserves_pixels(prefix in any::<[u8; 3]>(), suffix in any::<[u8; 3]>()) {
        let mut pixels = prefix.to_vec();
        pixels.extend_from_slice(b"endstream");
        pixels.extend_from_slice(&suffix);
        let data = compressed(&pixels, flate2::Compression::none());
        prop_assert_eq!(pixels_from_pdf(&image_pdf(&data, 5, true)), pixels);
    }
}
