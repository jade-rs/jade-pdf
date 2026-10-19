//! stencilを色画像と区別し、塗り色・透明度を独立した画素期待値で固定する。
#[path = "support/ccitt_mask.rs"]
mod codec;
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// 正式xrefの二画素stencilを公開APIへ渡す。
fn parse(attributes: &str, data: &[u8], content: &[u8]) -> jade_pdf2docx_parser::ParsedDocument {
    let pdf = fixture::source_image_pdf_with_content(attributes, data, "", 1, content);
    parse_pdf_bytes_with_options("stencil.pdf", &pdf, &ParseOptions::default()).unwrap()
}

/// 同じstencilもDoごとの塗り色で保存し、q/Qは色と行列を同時に復元する。
#[test]
fn repeated_stencil_uses_each_draw_color_and_transparency() {
    for decode in ["", "/Decode [0 1]", "/Decode [1 0]", "/Decode 8 0 R"] {
        let document = parse(
            &format!("/ImageMask true /Filter /FlateDecode {decode}"),
            &fixture::compressed(&[0x40]),
            b"1 0 0 rg /Im Do q 0 1 0 rg /Im Do Q /Im Do .5 g /Im Do /DeviceRGB cs .25 .5 .75 sc /Im Do",
        );
        assert_eq!(
            document.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        assert_eq!(document.image_elements.len(), 5);
        let alpha = if decode.contains("1 0") || decode.contains("8 0 R") {
            [0, 255]
        } else {
            [255, 0]
        };
        for (image, rgb) in document.image_elements.iter().zip([
            [255, 0, 0],
            [0, 255, 0],
            [255, 0, 0],
            [128, 128, 128],
            [64, 128, 191],
        ]) {
            assert_eq!(
                fixture::scanline(&image.png_bytes),
                [
                    0, rgb[0], rgb[1], rgb[2], alpha[0], rgb[0], rgb[1], rgb[2], alpha[1]
                ]
            );
        }
    }
}

/// CCITTのfilter極性をsampleへ戻してから、mask Decodeだけをalphaへ写す。
#[test]
fn ccitt_stencil_keeps_filter_polarity_separate_from_alpha() {
    for black_is_one in [false, true] {
        for inverse in [false, true] {
            let document = parse(
                &format!(
                    "/ImageMask true /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 2 /BlackIs1 {black_is_one} >> /Decode {}",
                    if inverse { "[1 0]" } else { "[0 1]" }
                ),
                &codec::encoded_mask(&[vec![true, false]]),
                b".25 .5 .75 rg /Im Do",
            );
            assert_eq!(
                document.quality.issues,
                [QualityIssue::OcrRequired { page: 0 }]
            );
            let first = if black_is_one == inverse { 255 } else { 0 };
            assert_eq!(
                fixture::scanline(&document.image_elements[0].png_bytes),
                [0, 64, 128, 191, first, 64, 128, 191, 255 - first]
            );
        }
    }
}

proptest! {
    /// packed sampleの各bitは、paddingや色成分に依存せずalphaへ写る。
    #[test]
    fn packed_stencil_alpha_matches_bit_model(first in any::<bool>(), second in any::<bool>(), inverse in any::<bool>(), red in any::<u8>(), green in any::<u8>(), blue in any::<u8>(), padding in 0u8..64) {
        let data = (u8::from(first)<<7)|(u8::from(second)<<6)|padding;
        let content = format!("{} {} {} rg /Im Do",f64::from(red)/255.0,f64::from(green)/255.0,f64::from(blue)/255.0);
        let document = parse(&format!("/ImageMask true /Filter /FlateDecode /Decode {}",if inverse { "[1 0]" } else { "[0 1]" }), &fixture::compressed(&[data]), content.as_bytes());
        prop_assert_eq!(document.quality.issues, vec![QualityIssue::OcrRequired { page: 0 }]);
        let alpha = |bit| if bit == inverse { 255 } else { 0 };
        prop_assert_eq!(fixture::scanline(&document.image_elements[0].png_bytes), [0,red,green,blue,alpha(first),red,green,blue,alpha(second)]);
    }
}

/// escapeされたDevice色空間名も同じ塗り色として解決する。
#[test]
fn escaped_device_color_space_is_equivalent() {
    let document = parse(
        "/ImageMask true /Filter /FlateDecode",
        &fixture::compressed(&[0x40]),
        b"/Device#52GB cs .25 .5 .75 sc /Im Do",
    );
    assert_eq!(
        document.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert_eq!(
        fixture::scanline(&document.image_elements[0].png_bytes),
        [0, 64, 128, 191, 255, 64, 128, 191, 0]
    );
}

/// 未対応の色・透明状態を推測せず、q/Q後の有効な塗り色は保持する。
#[test]
fn unsupported_paint_is_diagnosed_per_draw() {
    for unsupported in ["/Pattern cs /P scn", "/Unresolved gs", "0 0 0 1 k"] {
        let document = parse(
            "/ImageMask true /Filter /FlateDecode",
            &fixture::compressed(&[0x40]),
            format!("1 0 0 rg q {unsupported} /Im Do Q /Im Do").as_bytes(),
        );
        assert!(
            document
                .quality
                .issues
                .iter()
                .any(|issue| matches!(issue, QualityIssue::ImageReplaced { .. }))
        );
        assert_eq!(
            fixture::scanline(&document.image_elements[1].png_bytes),
            [0, 255, 0, 0, 255, 255, 0, 0, 0]
        );
    }
}

/// stencilの不正な属性や画素不足はstrictで保存前に拒否する。
#[test]
fn malformed_stencil_is_rejected_in_strict_mode() {
    for attributes in [
        "/ImageMask true /Decode [.1 1]",
        "/ImageMask true /ColorSpace /DeviceGray",
        "/ImageMask true /SMask 5 0 R",
        "/ImageMask 42",
        "/ImageMask true /Decode [0 1 0 1]",
    ] {
        let pdf = fixture::source_image_pdf_with_content(
            &format!("{attributes} /Filter /FlateDecode"),
            &fixture::compressed(&[0x40]),
            "",
            1,
            b"/Im Do",
        );
        assert!(
            matches!(parse_pdf_bytes_with_options("invalid.pdf", &pdf, &ParseOptions { strict:true, ..Default::default() }), Err(jade_pdf2docx_parser::ParseError::QualityRejected { report, .. }) if report.issues.iter().any(|issue|matches!(issue, QualityIssue::ImageReplaced { .. })))
        );
    }
}

/// byte境界外の行paddingを画素と混同せず、次の行のMSBから復号する。
#[test]
fn packed_rows_restart_at_byte_boundary() {
    let pdf = fixture::source_image_pdf_with_dimensions(
        "/ImageMask true /Filter /FlateDecode",
        &fixture::compressed(&[0x7f, 0x80]),
        "",
        1,
        b"/Im Do",
        2,
        2,
    );
    let document =
        parse_pdf_bytes_with_options("rows.pdf", &pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        document.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert_eq!(
        fixture::scanline(&document.image_elements[0].png_bytes),
        [0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255]
    );
}

/// 1 bit以外の宣言や不足sampleを灰色画像へfallbackせず品質診断する。
#[test]
fn invalid_component_width_and_short_samples_are_diagnosed() {
    for (bits, samples) in [(8, vec![0x40]), (1, vec![])] {
        let pdf = fixture::source_image_pdf_with_content(
            "/ImageMask true /Filter /FlateDecode",
            &fixture::compressed(&samples),
            "",
            bits,
            b"/Im Do",
        );
        let document =
            parse_pdf_bytes_with_options("invalid.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert!(
            document
                .quality
                .issues
                .iter()
                .any(|issue| matches!(issue, QualityIssue::ImageReplaced { .. }))
        );
    }
}
