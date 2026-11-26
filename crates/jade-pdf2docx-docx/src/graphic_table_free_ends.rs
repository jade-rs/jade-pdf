//! 表の内部列として採用する縦線の端点が、実在する罫線へ接続しているか確認する。
use super::{ParsedBounds, TABLE_RULE_JOIN_TOLERANCE_PT};

/// 複数の実在する横罫線を横切る自由端の棒を、部分罫線の列境界と断定しない。
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
            // 横線のない見出し内で止まる区切りや二重罫線の中断は従来の結合表経路へ残す。
            let crossings = ys[1..ys.len() - 1]
                .iter()
                .filter(|&&row| {
                    let y = row as f32;
                    y > line.y0 + tolerance
                        && y < line.y1 - tolerance
                        && horizontal.iter().any(|h| {
                            (h.y0 - y).abs() <= 1.
                                && h.x0 <= line.x0 + tolerance
                                && h.x1 >= line.x0 - tolerance
                        })
                })
                .take(2)
                .count();
            if crossings < 2 {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 同じ格子上で、実在する横線と途中までの縦線を作る。
    fn geometry() -> (Vec<ParsedBounds>, Vec<ParsedBounds>) {
        let vertical = vec![ParsedBounds {
            x0: 100.,
            x1: 100.,
            y0: 0.,
            y1: 250.,
        }];
        let horizontal = [0., 100., 200., 300.]
            .into_iter()
            .map(|y| ParsedBounds {
                x0: 0.,
                x1: 200.,
                y0: y,
                y1: y,
            })
            .collect();
        (vertical, horizontal)
    }

    /// 二つの横線を横切って止まる棒は、そのまま列境界へ採用しない。
    #[test]
    fn crossing_free_end_is_not_a_column_boundary() {
        let (v, h) = geometry();
        assert!(has_unanchored_column(
            &[0, 100, 200],
            &[0, 100, 200, 300],
            &v,
            &h
        ));
    }

    /// 列位置へ届かない同じ高さの横線を、端点の接続として扱わない。
    #[test]
    fn distant_horizontal_does_not_anchor_the_endpoint() {
        let (v, mut h) = geometry();
        h.push(ParsedBounds {
            x0: 0.,
            x1: 50.,
            y0: 250.,
            y1: 250.,
        });
        assert!(has_unanchored_column(
            &[0, 100, 200],
            &[0, 100, 200, 300],
            &v,
            &h
        ));
    }

    /// 分割線の継続や実在する部分横罫線へ接続した端点は、自由端として数えない。
    #[test]
    fn actual_junction_or_collinear_continuation_keeps_a_boundary() {
        let (v, h) = geometry();
        let mut joined = h.clone();
        joined.push(ParsedBounds {
            x0: 80.,
            x1: 120.,
            y0: 250.,
            y1: 250.,
        });
        assert!(!has_unanchored_column(
            &[0, 100, 200],
            &[0, 100, 200, 300],
            &v,
            &joined
        ));
        let mut continued = v;
        continued.push(ParsedBounds {
            x0: 100.,
            x1: 100.,
            y0: 250.,
            y1: 300.,
        });
        assert!(!has_unanchored_column(
            &[0, 100, 200],
            &[0, 100, 200, 300],
            &continued,
            &h
        ));
    }
}
