//! 段落領域と実段組の原点をPDF座標へ合わせる。
use super::{DEFAULT_DOCX_COLUMN_SPACE_TWIPS, pdf_points_to_indent_twips, pdf_points_to_twips};

/// 実sectionと同じ丸め後の幅・左余白から、右余白0の均等段原点をtwipで返す。
pub(super) fn column_origin_twips(
    page_width: f32,
    page_origin_x: f32,
    column_index: usize,
    column_count: usize,
) -> Option<f64> {
    if column_count < 2 || column_index >= column_count {
        return None;
    }
    let width = f64::from(u32::try_from(pdf_points_to_twips(page_width)?).ok()?);
    let left = f64::from(pdf_points_to_indent_twips(page_origin_x)?);
    let column_space = f64::from(DEFAULT_DOCX_COLUMN_SPACE_TWIPS);
    let available = width - left - column_space * (column_count - 1) as f64;
    if available <= 0.0 {
        return None;
    }
    Some(left + column_index as f64 * (available / column_count as f64 + column_space))
}

/// ページ内の原本行を、実段組原点からの符号つきindentへ変換する。
/// 負値は段間への正当な張り出しを保ち、未確認・ページ外の座標は従来処理へ戻す。
pub(super) fn column_indent(left: f32, right: f32, origin_twips: f64, width: f32) -> Option<i32> {
    if ![left, right, width].iter().all(|v| v.is_finite())
        || !origin_twips.is_finite()
        || left < 0.0
        || right <= left
        || right > width
        || origin_twips < 0.0
        || origin_twips > f64::from((width * 20.0).round())
    {
        return None;
    }
    let offset = (f64::from(left) * 20.0 - origin_twips).round();
    if offset < f64::from(i32::MIN) || offset > f64::from(i32::MAX) {
        return None;
    }
    Some(offset as i32)
}

/// 従来の書き込み幅を保ったまま実中心へ移動する符号つき左右indentを返す。
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
    let origin = f64::from((origin * 20.0).round());
    let width = f64::from((width * 20.0).round());
    if width > f64::from(i32::MAX) || width - 2.0 * origin < 1.0 {
        return None;
    }
    // 中央寄せの従来領域[origin, width-origin]を左右同じ量だけ平行移動する。
    // 片側だけの縮小は、代替フォントによる新たな折り返しを起こす。
    let shift = (((f64::from(left) + f64::from(right)) * 20.0 - width) / 2.0).round();
    Some((shift as i32, (origin - shift) as i32))
}

#[cfg(test)]
mod tests {
    use super::{centred_indents, column_indent, column_origin_twips};
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
            [450.0, 550.0, 400.0, 612.0],
            [0.0, 0.01, 0.0, 0.01],
            [100.0, 200.0, 20.0, f32::MAX],
        ] {
            let [left, right, origin, width] = geometry;
            assert_eq!(centred_indents(left, right, origin, width), None);
        }
    }

    /// 無効な段数・幅・余白や、空の段領域を有効な原点へ丸めない。
    #[test]
    fn invalid_columns_keep_existing_fallback() {
        for (width, left, index, count) in [
            (612., 48., 0, 0),
            (612., 48., 0, 1),
            (612., 48., 2, 2),
            (f32::NAN, 48., 1, 2),
            (f32::INFINITY, 48., 1, 2),
            (f32::MAX, 48., 1, 2),
            (612., f32::NAN, 1, 2),
            (612., -1., 1, 2),
            (0., 0., 1, 2),
            (48., 48., 1, 2),
            (612., 48., 1, usize::MAX),
        ] {
            assert_eq!(column_origin_twips(width, left, index, count), None);
        }
        for (left, right, origin, width) in [
            (f32::NAN, 100., 0., 612.),
            (10., f32::INFINITY, 0., 612.),
            (10., 100., f64::NAN, 612.),
            (10., 100., 0., f32::NAN),
            (-1., 100., 0., 612.),
            (100., 100., 0., 612.),
            (100., 99., 0., 612.),
            (100., 700., 0., 612.),
            (10., 100., -1., 612.),
            (10., 100., 13000., 612.),
            (0., 1., 1.0e10, 1.0e9),
            (1.0e9, 1.1e9, 0., 1.2e9),
        ] {
            assert_eq!(column_indent(left, right, origin, width), None);
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
            let word_left = f64::from((origin * 20.0).round()) + f64::from(indent_left);
            let word_right = f64::from((width * 20.0).round()) - f64::from(indent_right);
            let word_centre = (word_left + word_right) / 40.0;
            let source_centre = (f64::from(left) + f64::from(right)) / 2.0;
            prop_assert!((word_centre - source_centre).abs() <= 0.025001);
            prop_assert!(word_right > word_left);
            let legacy_width = f64::from((width * 20.0).round())
                - 2.0 * f64::from((origin * 20.0).round());
            prop_assert_eq!(word_right - word_left, legacy_width);
        }
    }
}
