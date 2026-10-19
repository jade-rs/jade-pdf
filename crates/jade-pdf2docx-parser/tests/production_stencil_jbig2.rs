//! JBIG2 stencilのcodec極性・Decode・行境界を独立画素で検証する。
#[path = "support/jbig2_mask.rs"]
mod codec;
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};

/// MMRを含むJBIG2も0を塗り、反転Decodeだけが塗る側を反転する。
#[test]
fn jbig2_stencil_preserves_fill_alpha_and_row_order() {
    let rows = vec![
        vec![true, false, true, true, false, false, true],
        vec![false, true, false, false, true, true, false],
    ];
    for (decode, inverse) in [
        ("", false),
        ("[0 1]", false),
        ("[1 0]", true),
        ("8 0 R", true),
    ] {
        let attributes = format!(
            "/ImageMask true /Filter /JBIG2Decode {}",
            if decode.is_empty() {
                String::new()
            } else {
                format!("/Decode {decode}")
            }
        );
        let pdf = fixture::source_image_pdf_with_dimensions(
            &attributes,
            &codec::encoded_mask(&rows),
            "",
            1,
            b"1 0 0 rg /Im Do",
            7,
            2,
        );
        let document =
            parse_pdf_bytes_with_options("jbig2.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert_eq!(
            document.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        let png = &document.image_elements[0].png_bytes;
        assert_eq!(png[25], 6);
        let mut expected = Vec::new();
        for row in &rows {
            expected.push(0);
            for black in row {
                expected.extend_from_slice(&[255, 0, 0, if *black != inverse { 255 } else { 0 }]);
            }
        }
        assert_eq!(fixture::scanline(png), expected, "Decode {decode}");
    }
}

/// pageと画像辞書の寸法差・破損streamは画素を補完して成功扱いにしない。
#[test]
fn jbig2_stencil_rejects_mismatched_dimensions_and_corruption() {
    let encoded = codec::encoded_mask(&[vec![true, false]]);
    for (width, height, data) in [
        (1, 1, encoded.as_slice()),
        (3, 1, &encoded),
        (2, 2, &encoded),
        (2, 1, &encoded[..12]),
    ] {
        let pdf = fixture::source_image_pdf_with_dimensions(
            "/ImageMask true /Filter /JBIG2Decode",
            data,
            "",
            1,
            b"/Im Do",
            width,
            height,
        );
        let document =
            parse_pdf_bytes_with_options("bad-jbig2.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert!(
            document
                .quality
                .issues
                .iter()
                .any(|issue| matches!(issue, QualityIssue::ImageReplaced { .. }))
        );
        assert!(
            parse_pdf_bytes_with_options(
                "bad-jbig2.pdf",
                &pdf,
                &ParseOptions {
                    strict: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
