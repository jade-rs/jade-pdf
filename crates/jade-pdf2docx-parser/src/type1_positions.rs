//! 確認済みの正立文字の間隔を、source fontを割り当てない論理空白へ戻す。
use crate::ParsedBounds;

/// 同じ正立baselineで正の距離を持つ二矩形だけから、空白のページ座標を求める。
pub(super) fn gap(
    previous: ParsedBounds,
    next: ParsedBounds,
    matrix: [f32; 6],
) -> Option<ParsedBounds> {
    if !matrix.iter().all(|v| v.is_finite())
        || matrix[0] <= 0.
        || matrix[3] <= 0.
        || matrix[1] != 0.
        || matrix[2] != 0.
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
