//! 暗黙の400万画素cutoffによる欠落を防ぎ、独立期待値と明示的予算を検証する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{
    ParseError, ParseLimits, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use std::io::Read;

/// filter 0のPNGを展開し、全scanlineを画素単位で検査する。
fn scanlines(png: &[u8]) -> Vec<u8> {
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
    raw
}

/// 400万画素境界の両側でGray Flateを保持し、明示pixel上限だけで拒否する。
#[test]
fn large_flate_pixels_survive_with_inclusive_explicit_limit() {
    for width in [2000u32, 2001] {
        let height = 2000;
        let samples: Vec<_> = (0..width * height)
            .map(|index| (index % 251) as u8)
            .collect();
        let pdf = fixture::source_image_pdf_with_dimensions(
            "/ColorSpace /DeviceGray /Filter /FlateDecode",
            &fixture::compressed(&samples),
            "",
            8,
            b"q 20 0 0 10 0 0 cm /Im Do Q",
            width,
            height,
        );
        let options = ParseOptions {
            limits: ParseLimits {
                max_image_pixels: u64::from(width) * u64::from(height),
                ..Default::default()
            },
            ..Default::default()
        };
        let parsed = parse_pdf_bytes_with_options("large.pdf", &pdf, &options).unwrap();
        assert_eq!(
            parsed.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        assert_eq!(parsed.image_elements.len(), 1);
        let png = &parsed.image_elements[0].png_bytes;
        assert_eq!(
            &png[16..24],
            [width.to_be_bytes(), height.to_be_bytes()].concat()
        );
        assert_eq!(png[25], 2);
        let raw = scanlines(png);
        assert_eq!(raw.len(), (width as usize * 3 + 1) * height as usize);
        for (row, expected) in raw
            .chunks_exact(width as usize * 3 + 1)
            .zip(samples.chunks_exact(width as usize))
        {
            assert_eq!(row[0], 0);
            assert!(
                row[1..]
                    .chunks_exact(3)
                    .zip(expected)
                    .all(|(rgb, gray)| rgb == [*gray; 3])
            );
        }
        for strict in [false, true] {
            let mut rejected = options.clone();
            rejected.strict = strict;
            rejected.limits.max_image_pixels -= 1;
            assert!(matches!(
                parse_pdf_bytes_with_options("large.pdf", &pdf, &rejected),
                Err(ParseError::ResourceLimit {
                    resource: "image pixels",
                    ..
                })
            ));
        }
    }
}
