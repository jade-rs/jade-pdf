//! 本文領域の原点に依存しないheader/footerの水平基準位置を作る。
use super::{ParagraphAlignment, StructuredTextLine};
use docx_rs::PageMargin;

/// ページ内の原本座標を、実section余白からの符号つきindentへ変換する。
/// 中央行の書き込み幅を保ち、左右の行は本文余白内へ押し込めない。
pub(super) fn indents(
    line: &StructuredTextLine,
    width_twips: u32,
    margin: &PageMargin,
) -> Option<(i32, i32)> {
    let left = f64::from(line.left_x) * 20.;
    let right = f64::from(line.right_x) * 20.;
    let width = f64::from(width_twips);
    let body_left = f64::from(margin.left);
    let body_right = width - f64::from(margin.right);
    if !left.is_finite()
        || !right.is_finite()
        || left < 0.
        || right <= left
        || right > width + 0.5
        || margin.left < 0
        || margin.right < 0
        || body_right <= body_left
        || margin.gutter != 0
    {
        return None;
    }
    let (indent_left, indent_right) = match line.alignment {
        ParagraphAlignment::Left => (
            left.round() - body_left,
            (body_right - right.round()).min(0.),
        ),
        ParagraphAlignment::Right => (
            (left.round() - body_left).min(0.),
            body_right - right.round(),
        ),
        ParagraphAlignment::Center => {
            let shift = ((left + right - body_left - body_right) / 2.).round();
            (shift, -shift)
        }
    };
    [indent_left, indent_right]
        .iter()
        .all(|value| *value >= f64::from(i32::MIN) && *value <= f64::from(i32::MAX))
        .then_some((indent_left as i32, indent_right as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 書式やfontの測定に依存しない、原本anchorだけを持つ行を作る。
    fn line(left_x: f32, right_x: f32, alignment: ParagraphAlignment) -> StructuredTextLine {
        StructuredTextLine {
            top_y: 0.,
            left_x,
            right_x,
            alignment,
            runs: Vec::new(),
            split_from_row: false,
            text: "owned chrome".into(),
        }
    }

    /// 非有限・範囲外・未対応のmarginを、有効なWord座標に丸めない。
    #[test]
    fn invalid_geometry_keeps_existing_fallback() {
        let zero = PageMargin::new().left(0).right(0);
        for (left, right) in [
            (f32::NAN, 100.),
            (10., f32::INFINITY),
            (-1., 100.),
            (100., 100.),
            (100., 90.),
            (10., 613.),
        ] {
            assert_eq!(
                indents(&line(left, right, ParagraphAlignment::Center), 12240, &zero),
                None
            );
        }
        let valid = line(200., 300., ParagraphAlignment::Center);
        for margin in [
            zero.clone().left(-1),
            zero.clone().right(-1),
            zero.clone().left(6000).right(6240),
            zero.clone().gutter(1),
        ] {
            assert_eq!(indents(&valid, 12240, &margin), None);
        }
        assert_eq!(indents(&valid, 0, &zero), None);
    }

    proptest! {
        /// Wordの実余白と符号つきindentから独立にanchorを復元する。
        #[test]
        fn prop_word_anchor_matches_source(
            width in 2000u32..50000,
            left_margin_fraction in 0u32..35,
            right_margin_fraction in 0u32..35,
            left_fraction in 0.0f32..0.8,
            span_fraction in 0.01f32..1.0,
            alignment in 0u8..3,
        ) {
            let margin = PageMargin::new().left((width * left_margin_fraction / 100) as i32)
                .right((width * right_margin_fraction / 100) as i32);
            let left = width as f32 / 20. * left_fraction;
            let right = left + (width as f32 / 20. - left) * span_fraction;
            let alignment = match alignment { 0 => ParagraphAlignment::Left,
                1 => ParagraphAlignment::Center, _ => ParagraphAlignment::Right };
            let line = line(left, right, alignment);
            let (indent_left, indent_right) = indents(&line, width, &margin).unwrap();
            let word_left = f64::from(margin.left) + f64::from(indent_left);
            let word_right = f64::from(width) - f64::from(margin.right) - f64::from(indent_right);
            let (actual, expected) = match alignment {
                ParagraphAlignment::Left => (word_left, f64::from(left) * 20.),
                ParagraphAlignment::Center => ((word_left + word_right) / 2.,
                    (f64::from(left) + f64::from(right)) * 10.),
                ParagraphAlignment::Right => (word_right, f64::from(right) * 20.),
            };
            prop_assert!((actual - expected).abs() <= 0.5001);
            prop_assert!(word_right > word_left);
            if alignment == ParagraphAlignment::Center {
                prop_assert_eq!(word_right - word_left,
                    f64::from(width) - f64::from(margin.left) - f64::from(margin.right));
            }
        }
    }
}
