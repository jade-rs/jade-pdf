//! 表の内部列として採用する縦線の端点が、実在する罫線へ接続しているか確認する。
use super::{ParsedBounds, TABLE_RULE_JOIN_TOLERANCE_PT};

/// 自由端を持つ内部の棒を列境界へ昇格する候補は、部分罫線の表と断定しない。
pub(super) fn has_unanchored_column(
    xs: &[i32],
    ys: &[i32],
    vertical: &[ParsedBounds],
    horizontal: &[ParsedBounds],
) -> bool {
    if xs.len() < 3 || ys.len() < 2 {
        return false;
    }
    let bottom = ys[0] as f32;
    let top = ys[ys.len() - 1] as f32;
    let tolerance = TABLE_RULE_JOIN_TOLERANCE_PT as f32;
    xs[1..xs.len() - 1].iter().any(|&x| {
        vertical.iter().any(|line| {
            if (line.x0 - x as f32).abs() > 1. || line.y1 < bottom || line.y0 > top {
                return false;
            }
            [line.y0, line.y1].into_iter().any(|y| {
                y > bottom + tolerance
                    && y < top - tolerance
                    && !horizontal.iter().any(|h| {
                        (h.y0 - y).abs() <= tolerance
                            && h.x0 <= line.x0 + tolerance
                            && h.x1 >= line.x0 - tolerance
                    })
                    && !vertical.iter().any(|other| {
                        (other.x0 - line.x0).abs() <= 1.
                            && other.y0 <= y + tolerance
                            && other.y1 >= y - tolerance
                            && (other.y0 < y - tolerance && line.y0 == y
                                || other.y1 > y + tolerance && line.y1 == y)
                    })
            })
        })
    })
}
