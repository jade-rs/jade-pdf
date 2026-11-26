//! 軸平行線分を追加heap確保なしで整列・結合する。
use super::{ParsedGraphicElement, normalized_bounds};

/// 向き・ページ・固定座標・区間端の原本値を返し、近接した平行線を区別する。
fn key(line: &ParsedGraphicElement) -> (bool, usize, f32, f32, f32) {
    let b = line.bounds;
    if b.y0 == b.y1 {
        (false, line.page, b.y0, b.x0, b.x1)
    } else {
        (true, line.page, b.x0, b.y0, b.y1)
    }
}

/// 元配列のslotだけで軸・ページごとに連続区間を結合する。
pub(super) fn merge(mut lines: Vec<ParsedGraphicElement>) -> Vec<ParsedGraphicElement> {
    lines.retain_mut(|line| {
        let b = line.bounds;
        if ![b.x0, b.y0, b.x1, b.y1, line.stroke_width]
            .iter()
            .all(|v| v.is_finite())
        {
            return false;
        }
        line.bounds = normalized_bounds(b);
        let b = line.bounds;
        (b.y0 == b.y1 && b.x0 != b.x1) || (b.x0 == b.x1 && b.y0 != b.y1)
    });
    lines.sort_unstable_by(|a, b| {
        let a = key(a);
        let b = key(b);
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.total_cmp(&b.2))
            .then(a.3.total_cmp(&b.3))
            .then(a.4.total_cmp(&b.4))
    });
    let mut written = 0usize;
    for read in 0..lines.len() {
        let current = lines[read].clone();
        let (axis, page, fixed, start, end) = key(&current);
        if written > 0 {
            let previous = &mut lines[written - 1];
            let (old_axis, old_page, old_fixed, _, old_end) = key(previous);
            if (axis, page, fixed) == (old_axis, old_page, old_fixed)
                && previous.stroke_width == current.stroke_width
                && start <= old_end
            {
                if axis {
                    previous.bounds.y1 = old_end.max(end);
                } else {
                    previous.bounds.x1 = old_end.max(end);
                }
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
