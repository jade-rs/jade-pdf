//! 上下に積んだ表・枠が外側の縦罫線を共有するとき、その罫線を表ごとに切り分ける。
//!
//! 官報の財務諸表では、上の計算書の左右の縦罫線を下の表の外枠まで 1 本で引くことがある。
//! そのままでは上の枠と下の表が 1 つの罫線の連結成分になり、上の枠の本文や右の段の枠まで
//! 1 つの表に取り込んでしまう。表の一番外側の横罫線から先へ伸びる縦罫線が、その横罫線に
//! 接する縦罫線の少数だけなら、別の枠の罫線とみなしてそこで切る（外枠のない表のように
//! 列の縦罫線がそろって伸びる場合は切らない）。

use super::*;

/// 横罫線と縦罫線が接するとみなす距離（pt）。
const TOUCH_PT: f32 = 2.0;
/// 表の外へ伸びた部分を別の罫線とみなす最小の長さ（pt）。
const MIN_EXTENSION_PT: f32 = 24.0;
/// 切り分けた罫線どうしが再び連結しないよう空ける間隔（pt）。
const CUT_GAP_PT: f32 = 3.0;
/// 切り分けの判定に必要な、外側の横罫線に接する縦罫線の数。
const MIN_TOUCHING_VERTICALS: usize = 3;

/// 横罫線が縦罫線に接する（端で接する・交差する）か判定する。
fn touches(horizontal: &ParsedBounds, vertical: &ParsedBounds) -> bool {
    horizontal.x0 <= vertical.x0 + TOUCH_PT
        && horizontal.x1 >= vertical.x0 - TOUCH_PT
        && horizontal.y0 >= vertical.y0 - TOUCH_PT
        && horizontal.y0 <= vertical.y1 + TOUCH_PT
}

/// `edge` の横罫線（同じ高さで縦罫線に接するもの）から `above` 側へ伸びる縦罫線が、
/// その横罫線に接する縦罫線の少数（半数未満）だけか判定する。
fn lone_extension(
    edge_y: f32,
    vertical: &ParsedBounds,
    horizontals: &[ParsedBounds],
    verticals: &[ParsedBounds],
    above: bool,
) -> bool {
    let Some(edge) = horizontals
        .iter()
        .filter(|horizontal| {
            (horizontal.y0 - edge_y).abs() <= TOUCH_PT && touches(horizontal, vertical)
        })
        .max_by(|a, b| (a.x1 - a.x0).total_cmp(&(b.x1 - b.x0)))
    else {
        return false;
    };
    let touching = verticals
        .iter()
        .filter(|other| touches(edge, other))
        .collect::<Vec<_>>();
    let extending = touching
        .iter()
        .filter(|other| {
            if above {
                other.y1 - edge.y0 > MIN_EXTENSION_PT
            } else {
                edge.y0 - other.y0 > MIN_EXTENSION_PT
            }
        })
        .count();
    touching.len() >= MIN_TOUCHING_VERTICALS && extending * 2 < touching.len()
}

/// 表の外へ伸びた共有の縦罫線を、表の外枠の位置で切り分けた罫線群を返す。
pub(super) fn split_shared_vertical_rules(
    graphics: &[ParsedGraphicElement],
) -> Vec<ParsedGraphicElement> {
    let bounds = graphics
        .iter()
        .map(|graphic| normalized_bounds(graphic.bounds))
        .collect::<Vec<_>>();
    let horizontals = bounds
        .iter()
        .copied()
        .filter(is_horizontal_line)
        .collect::<Vec<_>>();
    let verticals = bounds
        .iter()
        .copied()
        .filter(is_vertical_line)
        .collect::<Vec<_>>();
    let mut out = Vec::with_capacity(graphics.len());
    for (graphic, vertical) in graphics.iter().zip(&bounds) {
        if !is_vertical_line(vertical) {
            out.push(graphic.clone());
            continue;
        }
        let touching_ys = horizontals
            .iter()
            .filter(|horizontal| touches(horizontal, vertical))
            .map(|horizontal| horizontal.y0)
            .collect::<Vec<_>>();
        let (Some(top), Some(bottom)) = (
            touching_ys.iter().copied().reduce(f32::max),
            touching_ys.iter().copied().reduce(f32::min),
        ) else {
            out.push(graphic.clone());
            continue;
        };
        let cut_above = vertical.y1 - top > MIN_EXTENSION_PT
            && lone_extension(top, vertical, &horizontals, &verticals, true);
        let cut_below = bottom - vertical.y0 > MIN_EXTENSION_PT
            && lone_extension(bottom, vertical, &horizontals, &verticals, false);
        if !cut_above && !cut_below {
            out.push(graphic.clone());
            continue;
        }
        let piece = |y0: f32, y1: f32| {
            let mut piece = graphic.clone();
            piece.bounds = ParsedBounds {
                x0: vertical.x0,
                y0,
                x1: vertical.x1,
                y1,
            };
            piece
        };
        let low = if cut_below { bottom } else { vertical.y0 };
        let high = if cut_above { top } else { vertical.y1 };
        if cut_below {
            out.push(piece(vertical.y0, bottom - CUT_GAP_PT));
        }
        out.push(piece(low, high));
        if cut_above {
            out.push(piece(top + CUT_GAP_PT, vertical.y1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の罫線。
    fn rule(x0: f32, y0: f32, x1: f32, y1: f32) -> ParsedGraphicElement {
        ParsedGraphicElement {
            page: 0,
            bounds: ParsedBounds { x0, y0, x1, y1 },
            stroke_width: 0.5,
            dashed: false,
        }
    }

    /// 下の表の外枠より上へ伸びる外側の縦罫線だけを、表の上端で切る。
    #[test]
    fn splits_outer_rules_shared_with_a_frame_above() {
        let mut graphics = vec![
            // 上の枠と下の表で共有する左右の縦罫線
            rule(10.0, 0.0, 10.0, 300.0),
            rule(200.0, 0.0, 200.0, 300.0),
            // 下の表の内側の縦罫線と横罫線
            rule(60.0, 0.0, 60.0, 100.0),
            rule(90.0, 0.0, 90.0, 100.0),
            rule(120.0, 0.0, 120.0, 100.0),
            rule(150.0, 0.0, 150.0, 100.0),
        ];
        for y in [0.0, 50.0, 100.0] {
            graphics.push(rule(10.0, y, 200.0, y));
        }
        let split = split_shared_vertical_rules(&graphics);
        let left = split
            .iter()
            .filter(|graphic| graphic.bounds.x0 == 10.0 && is_vertical_line(&graphic.bounds))
            .map(|graphic| (graphic.bounds.y0, graphic.bounds.y1))
            .collect::<Vec<_>>();
        assert_eq!(left, vec![(0.0, 100.0), (103.0, 300.0)]);
    }

    /// 外枠のない表のように列の縦罫線がそろって伸びる場合は切らない。
    #[test]
    fn keeps_rules_of_tables_without_outer_edges() {
        let mut graphics = Vec::new();
        for x in [10.0, 60.0, 120.0, 200.0] {
            graphics.push(rule(x, 0.0, x, 150.0));
        }
        for y in [0.0, 50.0, 100.0] {
            graphics.push(rule(10.0, y, 200.0, y));
        }
        assert_eq!(split_shared_vertical_rules(&graphics), graphics);
    }
}
