//! 軸平行線分を追加heap確保なしで整列・結合する。
use super::{ParsedGraphicElement, normalized_bounds};

/// 向き・ページ・固定座標・区間端を整数へ正規化した比較キー。
fn key(line: &ParsedGraphicElement) -> (bool, usize, i32, i32, i32) {
    let b = line.bounds;
    if b.y0 == b.y1 {
        (false, line.page, b.y0 as i32, b.x0 as i32, b.x1 as i32)
    } else {
        (true, line.page, b.x0 as i32, b.y0 as i32, b.y1 as i32)
    }
}

/// 元配列のslotだけで軸・ページごとに連続区間を結合する。
pub(super) fn merge(mut lines: Vec<ParsedGraphicElement>) -> Vec<ParsedGraphicElement> {
    lines.retain_mut(|line| {
        let b = normalized_bounds(line.bounds);
        if ![b.x0, b.y0, b.x1, b.y1, line.stroke_width]
            .iter()
            .all(|v| v.is_finite())
        {
            return false;
        }
        line.bounds.x0 = (b.x0.round() as i32) as f32;
        line.bounds.y0 = (b.y0.round() as i32) as f32;
        line.bounds.x1 = (b.x1.round() as i32) as f32;
        line.bounds.y1 = (b.y1.round() as i32) as f32;
        let b = line.bounds;
        (b.y0 == b.y1 && b.x0 != b.x1) || (b.x0 == b.x1 && b.y0 != b.y1)
    });
    lines.sort_unstable_by_key(key);
    let mut written = 0usize;
    for read in 0..lines.len() {
        let current = lines[read].clone();
        let (axis, page, fixed, start, end) = key(&current);
        if written > 0 {
            let previous = &mut lines[written - 1];
            let (old_axis, old_page, old_fixed, _, old_end) = key(previous);
            if (axis, page, fixed) == (old_axis, old_page, old_fixed)
                && i64::from(start) <= i64::from(old_end) + 1
            {
                if axis {
                    previous.bounds.y1 = old_end.max(end) as f32;
                } else {
                    previous.bounds.x1 = old_end.max(end) as f32;
                }
                previous.stroke_width = previous.stroke_width.max(current.stroke_width);
                continue;
            }
        }
        lines[written] = current;
        written += 1;
    }
    lines.truncate(written);
    lines.sort_unstable_by(|a, b| {
        a.page
            .cmp(&b.page)
            .then(a.bounds.x0.total_cmp(&b.bounds.x0))
            .then(a.bounds.y0.total_cmp(&b.bounds.y0))
            .then(a.bounds.x1.total_cmp(&b.bounds.x1))
            .then(a.bounds.y1.total_cmp(&b.bounds.y1))
    });
    lines
}
