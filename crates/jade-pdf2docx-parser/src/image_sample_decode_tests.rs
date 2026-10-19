//! 元画像のDecodeを、色変換・Predictor・Matteから独立した画素期待値で検査する。
use super::*;
use proptest::prelude::*;
use std::io::Write;

/// 色空間を持たない1 bit stencilでも明示identity Decodeを保持する。
#[test]
fn codec_identity_accepts_stencil_without_color_space() {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let pdf = b"%PDF-1.7\n8 0 obj\ntrue\nendobj\n9 0 obj\n1\nendobj\n10 0 obj\n[0 1]\nendobj\n";
    let streams = crate::structure::validate_structure(
        Path::new("stencil.pdf"),
        pdf,
        11,
        &ParseLimits::default(),
        &budget,
    )
    .unwrap();
    budget.cache_streams(pdf, &streams);
    for attributes in [
        "/ImageMask true /BitsPerComponent 1 /Decode [0.0 1.0]",
        "/ImageMask true /Decode [0 1]",
        "/ImageMask 8 0 R /BitsPerComponent 9 0 R /Decode 10 0 R",
    ] {
        let dictionary = format!("<< {attributes} >>");
        assert_eq!(
            image_sample_decode::require_codec_identity(&budget, pdf, dictionary.as_bytes()),
            Some(()),
            "{attributes}"
        );
    }
    for attributes in [
        "/ImageMask true /BitsPerComponent 8 /Decode [0 1]",
        "/ImageMask false /Decode [0 1]",
        "/ImageMask true /Decode [1 0]",
        "/ImageMask true /Decode [0 1 0 1]",
        "/ImageMask true /Decode [0 NaN]",
    ] {
        let dictionary = format!("<< {attributes} >>");
        assert!(
            image_sample_decode::require_codec_identity(&budget, pdf, dictionary.as_bytes())
                .is_none(),
            "{attributes}"
        );
    }
}

/// 指定した二画素のFlate画像を復号してRGBA画素を返す。
fn pixels(color: &str, extra: &str, samples: &[u8]) -> Option<Vec<u8>> {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let pdf = b"%PDF-1.7\n8 0 obj\n[9 0 R 10 0 R]\nendobj\n9 0 obj\n1\nendobj\n10 0 obj\n0\nendobj\n11 0 obj\nnull\nendobj\n";
    let streams = crate::structure::validate_structure(
        Path::new("decode.pdf"),
        pdf,
        12,
        &ParseLimits::default(),
        &budget,
    )
    .ok()?;
    budget.cache_streams(pdf, &streams);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(samples).ok()?;
    let bytes = encoder.finish().ok()?;
    let dictionary = format!(
        "<< /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace {color} /Filter /FlateDecode {extra} >>"
    );
    let png = decode_image_stream_as_png(&budget, pdf, dictionary.as_bytes(), &bytes, 2, 1)?;
    Some(decode_filter0_png_to_rgba(&budget, &png)?.rgba)
}

/// Grayの反転と直接・間接配列・間接scalarを同じ画素へ解決する。
#[test]
fn gray_inverse_resolves_direct_and_indirect_values() {
    for decode in ["[1 0]", "8 0 R", "[9 0 R 10 0 R]"] {
        assert_eq!(
            pixels("/DeviceGray", &format!("/Decode {decode}"), &[0, 85]),
            Some(vec![255, 255, 255, 255, 170, 170, 170, 255]),
            "{decode}"
        );
    }
}

/// RGBの各pairを独立して写像し、色空間の範囲外だけをclipする。
#[test]
fn rgb_decode_maps_each_component() {
    assert_eq!(
        pixels(
            "/DeviceRGB",
            "/Decode [1 0 .25 .75 2 -1]",
            &[0, 128, 255, 255, 0, 0]
        ),
        Some(vec![255, 128, 0, 255, 0, 64, 255, 255])
    );
}

/// 既定のIndexed Decodeはsampleを保持し、palette範囲へclipする。
#[test]
fn indexed_default_clips_to_high_value() {
    let palette = "[/Indexed /DeviceRGB 1 <ff00000000ff>]";
    assert_eq!(
        pixels(palette, "", &[0, 1]),
        Some(vec![255, 0, 0, 255, 0, 0, 255, 255])
    );
    assert_eq!(
        pixels(palette, "", &[255, 2]),
        Some(vec![0, 0, 255, 255, 0, 0, 255, 255])
    );
}

/// IndexedはDecode後に最寄り整数へ丸めてpaletteを引く。
#[test]
fn indexed_decode_precedes_palette_and_rounds() {
    let palette = "[/Indexed /DeviceRGB 1 <ff00000000ff>]";
    assert_eq!(
        pixels(palette, "/Decode [1 0]", &[0, 255]),
        Some(vec![0, 0, 255, 255, 255, 0, 0, 255])
    );
    assert_eq!(
        pixels(palette, "/Decode [0 1]", &[127, 128]),
        Some(vec![255, 0, 0, 255, 0, 0, 255, 255])
    );
}

/// 差分復元したsampleにDecodeを適用し、差分byteは変換しない。
#[test]
fn predictor_restores_before_decode() {
    assert_eq!(
        pixels(
            "/DeviceGray",
            "/Decode [1 0] /DecodeParms << /Predictor 11 /Columns 2 >>",
            &[1, 10, 20]
        ),
        Some(vec![245, 245, 245, 255, 225, 225, 225, 255])
    );
}

/// nullと省略は既定写像へ戻し、間接nullも同じに扱う。
#[test]
fn absent_and_null_decode_are_identity() {
    for extra in ["", "/Decode null", "/Decode 11 0 R", "/Decode [0 1]"] {
        assert_eq!(
            pixels("/DeviceGray", extra, &[0, 85]),
            Some(vec![0, 0, 0, 255, 85, 85, 85, 255])
        );
    }
}

/// pair数・型・数値構文・欠落参照が不正なら誤った画素を成功扱いにしない。
#[test]
fn malformed_decode_is_rejected() {
    for decode in [
        "[]",
        "[1]",
        "[1 0 1 0]",
        "false",
        "[NaN 0]",
        "[inf 0]",
        "[1e1 0]",
        "[true 0]",
        "[null 0]",
        "12 0 R",
        "[12 0 R 0]",
    ] {
        assert!(
            pixels("/DeviceGray", &format!("/Decode {decode}"), &[0, 85]).is_none(),
            "{decode}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意Gray8の反転は整数255との差であり、透明度を変えない。
    #[test]
    fn inverse_gray_matches_integer_model(a in any::<u8>(), b in any::<u8>()) {
        prop_assert_eq!(pixels("/DeviceGray", "/Decode [1 0]", &[a,b]), Some(vec![255-a,255-a,255-a,255,255-b,255-b,255-b,255]));
    }
}
