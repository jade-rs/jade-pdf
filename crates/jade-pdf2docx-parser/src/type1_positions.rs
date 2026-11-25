//! 確認済みの正立文字の間隔を、source fontを割り当てない論理空白へ戻す。
use crate::{ParsedBounds, text_baseline::upright as baseline};

/// 同じ正立baselineで正の距離を持つ二矩形だけから、空白のページ座標を求める。
pub(super) fn gap(
    previous: ParsedBounds,
    next: ParsedBounds,
    matrix: [f32; 6],
) -> Option<ParsedBounds> {
    if baseline(matrix).is_none()
        || ![
            previous.x1,
            previous.y0,
            previous.y1,
            next.x0,
            next.y0,
            next.y1,
        ]
        .iter()
        .all(|v| v.is_finite())
        || previous.y0 != next.y0
        || previous.y1 != next.y1
        || previous.y1 <= previous.y0
        || next.x0 <= previous.x1
    {
        return None;
    }
    Some(ParsedBounds {
        x0: previous.x1,
        x1: next.x0,
        y0: previous.y0,
        y1: previous.y1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 同一baselineの有界なglyph矩形を作る。
    fn bounds(x0: f32, x1: f32) -> ParsedBounds {
        ParsedBounds {
            x0,
            x1,
            y0: 20.,
            y1: 40.,
        }
    }

    proptest! {
        /// 正のgapだけをそのまま取り出し、回転・反転・重なりへ位置を捏造しない。
        #[test]
        fn upright_gap_preserves_endpoints(x in -1000f32..1000., width in 0.1f32..30., distance in 0.01f32..30.) {
            let left = bounds(x, x + width);
            let right = bounds(left.x1 + distance, left.x1 + distance + width);
            let identity = [1., 0., 0., 1., 0., 0.];
            let result = gap(left, right, identity).unwrap();
            prop_assert_eq!(result.x0, left.x1);
            prop_assert_eq!(result.x1, right.x0);
            for matrix in [[-1.,0.,0.,1.,0.,0.], [1.,0.1,0.,1.,0.,0.], [1.,0.,0.1,1.,0.,0.], [1.,0.,0.,-1.,0.,0.], [1.,0.,0.,1.,f32::NAN,0.]] {
                prop_assert!(gap(left, right, matrix).is_none());
                prop_assert!(baseline(matrix).is_none());
            }
            prop_assert!(gap(right, left, identity).is_none());
        }
    }
}
