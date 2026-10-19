//! JPEG/JBIG2のDecodeをsample段階へ適用し、Matteより前の精度を固定する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/jbig2_mask.rs"]
mod jbig2;
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// 白黒のJBIG2 sampleを、直接・間接・有限範囲外のGray端点へ写像する。
#[test]
fn jbig2_decode_maps_samples_before_png_encoding() {
    let encoded = jbig2::encoded_mask(&[vec![true, false]]);
    for (decode, gray) in [
        ("[1 0]", [255, 0]),
        ("[.25 .75]", [64, 191]),
        ("8 0 R", [255, 0]),
        ("[-1 2]", [0, 255]),
    ] {
        let pdf = fixture::source_image_pdf_with_bits(
            &format!("/ColorSpace /DeviceGray /Filter /JBIG2Decode /Decode {decode}"),
            &encoded,
            "",
            1,
        );
        assert_pixels(
            &pdf,
            &[0, gray[0], gray[0], gray[0], gray[1], gray[1], gray[1]],
        );
    }
}

/// JBIG2 Decodeの端点を8 bitへ丸めず、Matte逆変換の最後だけ丸める。
#[test]
fn jbig2_decode_and_matte_share_one_color_rounding() {
    let pdf = fixture::source_image_pdf_with_bits(
        "/ColorSpace /DeviceGray /Filter /JBIG2Decode /Decode [.3 .7] /SMask 5 0 R",
        &jbig2::encoded_mask(&[vec![true, false]]),
        "/Matte [.2]",
        1,
    );
    assert_pixels(&pdf, &[0, 128, 128, 128, 85, 179, 179, 179, 255]);
}

/// 本文と期待画素を保持し、画像置換を成功と見なさない共通assert。
fn assert_pixels(pdf: &[u8], expected: &[u8]) {
    let parsed = parse_pdf_bytes_with_options("codec.pdf", pdf, &ParseOptions::default()).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert!(parsed.text_preview.contains("retained text"));
    assert_eq!(parsed.image_elements.len(), 1);
    assert_eq!(
        fixture::scanline(&parsed.image_elements[0].png_bytes),
        expected
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 有限端点のclipと量子化を、白黒sampleの整数oracleで照合する。
    #[test]
    fn jbig2_endpoints_match_integer_oracle(low in -256i32..512, high in -256i32..512) {
        let endpoint = |n: i32| format!("{}{}.{:08}", if n < 0 {"-"} else {""}, n.abs()/256, (n.abs()%256)*390625);
        let pdf = fixture::source_image_pdf_with_bits(
            &format!("/ColorSpace /DeviceGray /Filter /JBIG2Decode /Decode [{} {}]",endpoint(low),endpoint(high)),
            &jbig2::encoded_mask(&[vec![true,false]]), "", 1,
        );
        let gray = |n: i32| ((255*n.clamp(0,256)+128)/256) as u8;
        assert_pixels(&pdf, &[0,gray(low),gray(low),gray(low),gray(high),gray(high),gray(high)]);
    }
}

/// JBIG2の既定/identity Decodeにも検証済みsourceのMatteを許可する。
#[test]
fn jbig2_default_and_identity_decode_apply_matte() {
    for decode in ["", "/Decode null", "/Decode [0 1]"] {
        let pdf = fixture::source_image_pdf_with_bits(
            &format!("/ColorSpace /DeviceGray /Filter /JBIG2Decode /SMask 5 0 R {decode}"),
            &jbig2::encoded_mask(&[vec![true, false]]),
            "/Matte [.2]",
            1,
        );
        assert_pixels(&pdf, &[0, 0, 0, 0, 85, 255, 255, 255, 255]);
    }
}
