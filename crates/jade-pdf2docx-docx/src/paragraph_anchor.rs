//! 中央寄せの段落領域をPDFの実中心へ合わせる。

/// ページ中央への吸着を避け、実中心を保つ非負の左右indentを返す。
/// 本文の右ページ余白が0の配置専用。寸法不明・範囲外なら従来処理へ戻す。
pub(super) fn centred_indents(
    left: f32,
    right: f32,
    origin: f32,
    width: f32,
) -> Option<(i32, i32)> {
    if ![left, right, origin, width].iter().all(|v| v.is_finite())
        || origin < 0.0
        || left < origin
        || right <= left
        || right > width
    {
        return None;
    }
    // セクション寸法と同じtwip丸めを先に行い、原点の丸め誤差を重ねない。
    let origin = (f64::from(origin) * 20.0).round();
    let width = (f64::from(width) * 20.0).round();
    if width > f64::from(i32::MAX) {
        return None;
    }
    let correction = (origin + width - (f64::from(left) + f64::from(right)) * 20.0).round();
    if width - origin - correction.abs() < 1.0 {
        return None;
    }
    if correction >= 0.0 {
        Some((0, correction as i32))
    } else {
        Some((-correction as i32, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::centred_indents;
    use proptest::prelude::*;

    /// 未確認・範囲外の座標から有効なindentを作らない。
    #[test]
    fn invalid_geometry_uses_existing_fallback() {
        for geometry in [
            [f32::NAN, 200.0, 20.0, 612.0],
            [100.0, f32::INFINITY, 20.0, 612.0],
            [100.0, 200.0, f32::NEG_INFINITY, 612.0],
            [100.0, 200.0, 20.0, f32::NAN],
            [100.0, 200.0, -1.0, 612.0],
            [10.0, 200.0, 20.0, 612.0],
            [100.0, 100.0, 20.0, 612.0],
            [200.0, 100.0, 20.0, 612.0],
            [100.0, 700.0, 20.0, 612.0],
            [0.0, 0.01, 0.0, 0.01],
            [100.0, 200.0, 20.0, f32::MAX],
        ] {
            let [left, right, origin, width] = geometry;
            assert_eq!(centred_indents(left, right, origin, width), None);
        }
    }

    proptest! {
        /// 独立したWord領域の式で、任意の位置・幅の中心と非空領域を検証する。
        #[test]
        fn quantised_word_region_keeps_source_centre(
            width in 200.0_f32..1500.0,
            origin_fraction in 0.0_f32..0.2,
            left_fraction in 0.0_f32..0.8,
            span_fraction in 0.01_f32..1.0,
        ) {
            let origin = width * origin_fraction;
            let left = origin + (width - origin) * left_fraction;
            let right = left + (width - left) * span_fraction;
            let (indent_left, indent_right) = centred_indents(left, right, origin, width).unwrap();
            let word_left = (f64::from(origin) * 20.0).round() + f64::from(indent_left);
            let word_right = (f64::from(width) * 20.0).round() - f64::from(indent_right);
            let word_centre = (word_left + word_right) / 40.0;
            let source_centre = (f64::from(left) + f64::from(right)) / 2.0;
            prop_assert!((word_centre - source_centre).abs() <= 0.012501);
            prop_assert!(word_right > word_left);
            prop_assert!(indent_left >= 0 && indent_right >= 0);
            prop_assert!(indent_left == 0 || indent_right == 0);
        }
    }
}
