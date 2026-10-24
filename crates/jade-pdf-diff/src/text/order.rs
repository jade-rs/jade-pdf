//! Geometric reading order for horizontal text. Recursive whitespace cuts let
//! a spanning title/footer coexist with columns. This is not tagged-PDF order.
use super::ExtractedRun;

pub(super) fn sort_runs(runs: &mut [ExtractedRun], point: f64) {
    cut(runs, point, 0);
}

fn cut(runs: &mut [ExtractedRun], point: f64, depth: usize) {
    if runs.len() < 2 {
        return;
    }
    // Bound sorting/work and call-stack growth on adversarial layouts.
    if depth < 32 {
        let mut sizes: Vec<_> = runs.iter().filter_map(|r| r.size).collect();
        sizes.sort_unstable_by(f64::total_cmp);
        let em = sizes
            .get(sizes.len() / 2)
            .copied()
            .unwrap_or(12.0 * point)
            .max(point);
        let column_gap = (em * 1.5).max(2.0 * point);
        let row_cuts = row_gaps(runs, em * 1.5);
        for &(width, y) in row_cuts.iter().take(8) {
            // Origins are baseline positions. Ordinary leading must not look
            // like a new grid when a bank/address row contains fewer labels.
            if width <= em * 2.0 {
                continue;
            }
            // A clear horizontal band can separate different column grids,
            // e.g. four author cards followed by three centered cards. Do not
            // read the first column through both grids before its neighbors.
            // Equal column counts retain column-major paragraph ordering.
            let top = columns(runs.iter().filter(|r| r.layout.y1 > y), column_gap);
            let bottom = columns(runs.iter().filter(|r| r.layout.y1 <= y), column_gap);
            if top.len() > 1
                && bottom.len() > 1
                && top.len() != bottom.len()
                && !continuing_columns(&top, &bottom, em)
            {
                split_rows(runs, y, point, depth);
                return;
            }
        }
        if let Some(x) = gap(runs, true, column_gap) {
            runs.sort_by(|a, b| a.layout.x0.total_cmp(&b.layout.x0));
            let split = runs.partition_point(|r| r.layout.x0 < x);
            if split > 0 && split < runs.len() {
                let (left, right) = runs.split_at_mut(split);
                // Disjoint vertical regions are sequential text, not parallel
                // columns (e.g. three diagonally positioned labels).
                if vertical_overlap(left, right, em * 0.35) {
                    cut(left, point, depth + 1);
                    cut(right, point, depth + 1);
                    return;
                }
            }
        }
        if let Some(&(_, y)) = row_cuts.first() {
            split_rows(runs, y, point, depth);
            return;
        }
    }
    super::inline::sort(runs, point);
}

fn split_rows(runs: &mut [ExtractedRun], y: f64, point: f64, depth: usize) {
    runs.sort_by(|a, b| b.layout.y1.total_cmp(&a.layout.y1));
    let split = runs.partition_point(|r| r.layout.y1 > y);
    debug_assert!(split > 0 && split < runs.len());
    let (top, bottom) = runs.split_at_mut(split);
    cut(top, point, depth + 1);
    cut(bottom, point, depth + 1);
}

fn columns<'a>(runs: impl Iterator<Item = &'a ExtractedRun>, minimum: f64) -> Vec<(f64, f64)> {
    let mut intervals: Vec<_> = runs.map(|r| (r.layout.x0, r.layout.x1)).collect();
    intervals.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut result: Vec<(f64, f64)> = Vec::new();
    for (start, finish) in intervals {
        if let Some(last) = result.last_mut().filter(|last| start - last.1 <= minimum) {
            last.1 = last.1.max(finish);
        } else {
            result.push((start, finish));
        }
    }
    result
}

fn continuing_columns(a: &[(f64, f64)], b: &[(f64, f64)], em: f64) -> bool {
    let (small, large) = if a.len() < b.len() { (a, b) } else { (b, a) };
    // A shorter column may simply finish early. Retain the remaining lanes
    // when their left anchors and next-column boundaries still match. Check
    // only neighboring anchors, keeping work linearithmic on sparse layouts.
    small.iter().all(|&(start, end)| {
        let at = large.partition_point(|&(left, _)| left <= start);
        (at.saturating_sub(1)..=at.min(large.len() - 1)).any(|i| {
            (start - large[i].0).abs() <= em * 1.5
                && large.get(i + 1).is_none_or(|next| end < next.0 - em * 0.35)
        })
    })
}

fn row_gaps(runs: &[ExtractedRun], minimum: f64) -> Vec<(f64, f64)> {
    let mut intervals: Vec<_> = runs.iter().map(|r| (r.layout.y0, r.layout.y1)).collect();
    intervals.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = intervals[0].1;
    let mut gaps = Vec::new();
    for (start, finish) in intervals.into_iter().skip(1) {
        if start - end > minimum {
            gaps.push((start - end, end / 2.0 + start / 2.0));
        }
        end = end.max(finish);
    }
    gaps.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.total_cmp(&b.1)));
    gaps
}

fn vertical_overlap(left: &[ExtractedRun], right: &[ExtractedRun], tolerance: f64) -> bool {
    let range = |runs: &[ExtractedRun]| {
        runs.iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), r| {
                (low.min(r.layout.y0), high.max(r.layout.y1))
            })
    };
    let (al, ah) = range(left);
    let (bl, bh) = range(right);
    al.max(bl) <= ah.min(bh) + tolerance
}

fn gap(runs: &[ExtractedRun], horizontal: bool, minimum: f64) -> Option<f64> {
    // Use PDF positioning origins, independent of substitute-font ink metrics.
    let mut intervals: Vec<_> = runs
        .iter()
        .map(|r| {
            if horizontal {
                (r.layout.x0, r.layout.x1)
            } else {
                (r.layout.y0, r.layout.y1)
            }
        })
        .collect();
    intervals.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    let mut end = intervals[0].1;
    let mut best = minimum;
    let mut cut = None;
    for (start, finish) in intervals.into_iter().skip(1) {
        let width = start - end;
        if width > best {
            best = width;
            cut = Some(end / 2.0 + start / 2.0);
        }
        end = end.max(finish);
    }
    cut
}
