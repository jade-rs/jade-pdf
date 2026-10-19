//! Flate画像の差分を画素として誤成功にしない回帰。
use super::*;
use flate2::{Compression, write::ZlibEncoder};
use proptest::prelude::*;

/// 差分符号化済みの小さな画像をPNGからRGBAへ復号する。
fn pixels(color: &str, parameters: &str, bytes: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    let dictionary = format!(
        "<< /BitsPerComponent 8 /ColorSpace {color} /Filter /FlateDecode /DecodeParms {parameters} >>"
    );
    let png = decode_image_stream_as_png(
        &budget,
        b"",
        dictionary.as_bytes(),
        &encoder.finish().unwrap(),
        width,
        height,
    )?;
    Some(decode_filter0_png_to_rgba(&budget, &png)?.rgba)
}

/// 固定RGB画素をopaqueなRGBA期待値にする。
fn rgba(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3)
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect()
}

/// PNGのUp差分と行頭タグを除いて、2行のRGBを保持する。
#[test]
fn png_up_preserves_rgb_rows() {
    let expected = rgba(&[10, 20, 30, 40, 50, 60, 11, 22, 33, 45, 55, 65]);
    for predictor in 10..=15 {
        let parameters = format!("<< /Predictor {predictor} /Colors 3 /Columns 2 >>");
        assert_eq!(
            pixels(
                "/DeviceRGB",
                &parameters,
                &[0, 10, 20, 30, 40, 50, 60, 2, 1, 2, 3, 5, 5, 5],
                2,
                2
            ),
            Some(expected.clone()),
            "{predictor}"
        );
    }
}

/// TIFFの差分は行境界でリセットし、同じ色成分を加算する。
#[test]
fn tiff_preserves_rgb_rows() {
    assert_eq!(
        pixels(
            "/DeviceRGB",
            "<< /Predictor 2 /Colors 3 /Columns 2 >>",
            &[10, 20, 30, 30, 30, 30, 11, 22, 33, 34, 33, 32],
            2,
            2
        ),
        Some(rgba(&[10, 20, 30, 40, 50, 60, 11, 22, 33, 45, 55, 65]))
    );
}

/// PNGの全方式を、encoderに依存しない固定差分で検証する。
#[test]
fn every_png_filter_preserves_rgb() {
    for (tag, first, second) in [
        (0, [10, 20, 30, 40, 50, 60], [11, 22, 33, 45, 55, 65]),
        (1, [10, 20, 30, 30, 30, 30], [11, 22, 33, 34, 33, 32]),
        (2, [10, 20, 30, 40, 50, 60], [1, 2, 3, 5, 5, 5]),
        (3, [10, 20, 30, 35, 40, 45], [6, 12, 18, 20, 19, 19]),
        (4, [10, 20, 30, 30, 30, 30], [1, 2, 3, 5, 5, 5]),
    ] {
        let mut bytes = vec![tag];
        bytes.extend_from_slice(&first);
        bytes.push(tag);
        bytes.extend_from_slice(&second);
        assert_eq!(
            pixels(
                "/DeviceRGB",
                "<< /Predictor 15 /Colors 3 /Columns 2 >>",
                &bytes,
                2,
                2
            ),
            Some(rgba(&[10, 20, 30, 40, 50, 60, 11, 22, 33, 45, 55, 65])),
            "{tag}"
        );
    }
}

/// GrayとIndexedは差分を復元した後にRGBへ変換する。
#[test]
fn gray_and_indexed_preserve_samples() {
    assert_eq!(
        pixels(
            "/DeviceGray",
            "<< /Predictor 11 /Columns 2 >>",
            &[1, 10, 20, 1, 40, 10],
            2,
            2
        ),
        Some(rgba(&[10, 10, 10, 30, 30, 30, 40, 40, 40, 50, 50, 50]))
    );
    assert_eq!(
        pixels(
            "[/Indexed /DeviceRGB 1 <0a141e28323c>]",
            "<< /Predictor 11 /Columns 2 >>",
            &[1, 0, 1],
            2,
            1
        ),
        Some(rgba(&[10, 20, 30, 40, 50, 60]))
    );
}

/// 型・範囲・行境界の破損をraw画素として成功させない。
#[test]
fn malformed_predictors_are_rejected() {
    for parameters in [
        "<< /Predictor 0 >>",
        "<< /Predictor 3 >>",
        "<< /Predictor true >>",
        "<< /Predictor 12.5 >>",
        "<< /Predictor 12 /Columns 0 >>",
        "<< /Predictor 12 /Columns -1 >>",
        "<< /Predictor 12 /Colors 0 >>",
        "<< /Predictor 12 /Colors false >>",
        "<< /Predictor 12 /BitsPerComponent 3 >>",
        "<< /Predictor 2 /BitsPerComponent 16 >>",
        "false",
    ] {
        assert!(
            pixels("/DeviceGray", parameters, &[10, 20], 2, 1).is_none(),
            "{parameters}"
        );
    }
    for bytes in [&[0, 10][..], &[0, 10, 20, 0], &[5, 10, 20]] {
        assert!(
            pixels("/DeviceGray", "<< /Predictor 15 /Columns 2 >>", bytes, 2, 1).is_none(),
            "{bytes:?}"
        );
    }
}

/// Noneのnull/defaultは画素を変更しない。
#[test]
fn no_prediction_defaults_preserve_pixels() {
    for parameters in [
        "null",
        "<< >>",
        "<< /Predictor null >>",
        "<< /Predictor +1 >>",
        "[null]",
    ] {
        assert_eq!(
            pixels("/DeviceRGB", parameters, &[10, 20, 30], 1, 1),
            Some(rgba(&[10, 20, 30]))
        );
    }
}

proptest! {
    /// 独立したSub符号化の任意RGB行は桁あふれも含めて元に戻る。
    #[test]
    fn sub_predictor_roundtrips_rgb(first in any::<[u8;3]>(), second in any::<[u8;3]>()) {
        let bytes = [1,first[0],first[1],first[2],second[0].wrapping_sub(first[0]),second[1].wrapping_sub(first[1]),second[2].wrapping_sub(first[2])];
        prop_assert_eq!(pixels("/DeviceRGB", "<< /Predictor 11 /Colors 3 /Columns 2 >>", &bytes, 2, 1), Some(rgba(&[first[0],first[1],first[2],second[0],second[1],second[2]])));
    }
}
