//! PDF 32000-1 §11.6.5.3のpreblend逆変換を独立した画素期待値で固定する。
use super::*;
use flate2::{Compression, write::ZlibEncoder};
use proptest::prelude::*;

/// 色と透明度の小さなFlate fixtureを復号する。
fn pixels(color: &str, matte: &str, samples: &[u8], alpha: &[u8], extra: &str) -> Option<Vec<u8>> {
    let compress = |bytes: &[u8]| {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    };
    let mask = compress(alpha);
    let mut pdf = format!("%PDF-1.7\n5 0 obj\n<< /Width {} /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode /Matte {matte} /Length {} >>\nstream\n", alpha.len(), mask.len()).into_bytes();
    pdf.extend_from_slice(&mask);
    pdf.extend_from_slice(b"\nendstream\nendobj\n6 0 obj\n[1 1 1]\nendobj\n");
    let budget = DecodeBudget::new(&ParseLimits::default());
    let streams = crate::structure::validate_structure(
        std::path::Path::new("matte.pdf"),
        &pdf,
        9,
        &ParseLimits::default(),
        &budget,
    )
    .unwrap();
    budget.cache_streams(&pdf, &streams);
    let dict = format!(
        "<< /BitsPerComponent 8 /ColorSpace {color} /Filter /FlateDecode /SMask 5 0 R {extra} >>"
    );
    let png = decode_image_stream_as_png(&budget, &pdf, dict.as_bytes(), &compress(samples), 2, 1)?;
    Some(decode_filter0_png_to_rgba(&budget, &png)?.rgba)
}

/// 白Matteを除き、透明度1/3の元RGBを正確に復元する。
#[test]
fn white_matte_recovers_original_rgb() {
    assert_eq!(
        pixels(
            "/DeviceRGB",
            "[1 1 1]",
            &[190, 210, 230, 255, 255, 255],
            &[85, 0],
            ""
        ),
        Some(vec![60, 120, 180, 85, 0, 0, 0, 0])
    );
    assert_eq!(
        pixels(
            "/DeviceRGB",
            "6 0 R",
            &[190, 210, 230, 255, 255, 255],
            &[85, 0],
            ""
        ),
        Some(vec![60, 120, 180, 85, 0, 0, 0, 0])
    );
}

/// 壊れたsource predictorを未復号のままMatte逆変換しない。
#[test]
fn malformed_source_predictor_rejects_matte() {
    assert!(
        pixels(
            "/DeviceRGB",
            "[0 0 0]",
            &[10; 6],
            &[85, 255],
            "/DecodeParms << /Predictor 12 >>"
        )
        .is_none()
    );
}

/// sourceのSub差分を復元してから黒Matteを逆変換する。
#[test]
fn predictor_is_recovered_before_matte() {
    assert_eq!(
        pixels(
            "/DeviceRGB",
            "[0 0 0]",
            &[1, 10, 20, 30, 30, 30, 30],
            &[85, 255],
            "/DecodeParms << /Predictor 11 /Colors 3 /Columns 2 >>"
        ),
        Some(vec![30, 60, 90, 85, 40, 50, 60, 255])
    );
}

/// GrayのMatteはGrayの元値を復元してからRGBへ写せる。
#[test]
fn gray_matte_recovers_original_gray() {
    assert_eq!(
        pixels("/DeviceGray", "[.2]", &[102, 204], &[85, 255], ""),
        Some(vec![204, 204, 204, 85, 204, 204, 204, 255])
    );
}

/// 量子化から範囲外となる逆変換をclampし、opaque画素は保持する。
#[test]
fn matte_clamps_and_keeps_opaque_pixels() {
    assert_eq!(
        pixels(
            "/DeviceRGB",
            "[1 0 .5]",
            &[0, 255, 128, 9, 23, 71],
            &[1, 255],
            "/Decode [0 1 0 1 0 1]"
        ),
        Some(vec![0, 255, 255, 1, 9, 23, 71, 255])
    );
}

/// 不正Matteと未対応のsource Decodeを暗黙に成功へ変えない。
#[test]
fn invalid_matte_and_source_decode_are_rejected() {
    for matte in [
        "[0 0]",
        "[0 0 0 0]",
        "[-.1 0 0]",
        "[1.1 0 0]",
        "[NaN 0 0]",
        "false",
    ] {
        assert!(
            pixels("/DeviceRGB", matte, &[10; 6], &[85, 255], "").is_none(),
            "{matte}"
        );
    }
    assert!(
        pixels(
            "/DeviceRGB",
            "[0 0 0]",
            &[10; 6],
            &[85, 255],
            "/Decode [1 0 0 1]"
        )
        .is_none()
    );
    assert!(pixels("/DeviceRGB", "[0 0 0]", &[10; 6], &[85], "").is_none());
    assert!(
        pixels(
            "/DeviceRGB",
            "[0 0 0] /SMask 5 0 R",
            &[10; 6],
            &[85, 255],
            ""
        )
        .is_none()
    );
    assert!(
        pixels(
            "[/Indexed /DeviceRGB 0 <0a0a0a>]",
            "[0 0 0]",
            &[0; 2],
            &[85, 255],
            ""
        )
        .is_none()
    );
}

/// 非identity DecodeをMatte逆変換の前に反映し、最終段階で一度だけ丸める。
#[test]
fn source_decode_precedes_matte_without_intermediate_rounding() {
    assert_eq!(
        pixels(
            "/DeviceGray",
            "[.2]",
            &[128, 64],
            &[85, 255],
            "/Decode [.1 .5]"
        ),
        Some(vec![128, 128, 128, 85, 51, 51, 51, 255])
    );
    assert_eq!(
        pixels(
            "/DeviceRGB",
            "[0 0 0]",
            &[170, 170, 170, 85, 85, 85],
            &[85, 255],
            "/Decode [1 0 1 0 1 0]"
        ),
        Some(vec![255, 255, 255, 85, 170, 170, 170, 255])
    );
}

/// 小数DecodeとMatteの丸め境界を、独立した整数式で全sample検査する。
#[test]
fn source_decode_matte_matches_exact_decimal_model_for_all_samples() {
    for first in (0u16..=254).step_by(2) {
        let samples = [first as u8, (first + 1) as u8];
        let mut expected = Vec::new();
        for sample in samples {
            // (0.2 + (0.1 + 0.4 * sample / 255 - 0.2) * 3) * 255
            // を小数演算なしで最近傍へ丸め、成分範囲へ収める。
            let gray = ((12 * i32::from(sample) - 250).div_euclid(10)).clamp(0, 255) as u8;
            expected.extend_from_slice(&[gray, gray, gray, 85]);
        }
        assert_eq!(
            pixels(
                "/DeviceGray",
                "[.2]",
                &samples,
                &[85, 85],
                "/Decode [.1 .5]"
            ),
            Some(expected),
            "source samples {samples:?}"
        );
    }
}

proptest! {
    /// 黒Matteで正確に表せる1/3透明のRGBは元値へ戻る。
    #[test]
    fn black_matte_recovers_exact_quantized_samples(a in 0u8..=85, b in 0u8..=85, c in 0u8..=85) {
        prop_assert_eq!(pixels("/DeviceRGB", "[0 0 0]", &[a,b,c,a,b,c], &[85,255], ""), Some(vec![a*3,b*3,c*3,85,a,b,c,255]));
    }
}

/// Rust実数の指数構文をPDF数値として受理しない。
#[test]
fn matte_rejects_exponent_number_syntax() {
    for matte in ["[1e-1 0 0]", "[0 1E-1 0]", "[0 0 0e0]"] {
        assert!(
            pixels("/DeviceRGB", matte, &[64; 6], &[85, 255], "").is_none(),
            "{matte}"
        );
    }
}
