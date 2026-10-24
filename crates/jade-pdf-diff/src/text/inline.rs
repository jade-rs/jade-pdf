//! Keep smaller, shifted inline fragments between their surrounding text.
//! This is geometric line order, not a mathematical expression parser.
use super::ExtractedRun;
use std::collections::BTreeMap;

/// A same-size baseline must not merge across a fragment on another baseline.
/// Answer all open-x/closed-y rectangle queries with an offline sweep and a
/// Fenwick tree, rather than scanning every glyph/run for every proposed join.
pub(super) fn barriers(rows: &[Vec<ExtractedRun>], point: f64) -> Vec<Vec<bool>> {
    let mut result: Vec<Vec<bool>> = rows
        .iter()
        .map(|row| vec![false; row.len().saturating_sub(1)])
        .collect();
    let mut points: Vec<_> = rows
        .iter()
        .flatten()
        .map(|r| {
            (
                r.origin.0,
                r.origin.1,
                r.size.unwrap_or(12.0 * point).max(point) * 0.6,
            )
        })
        .collect();
    points.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut ys: Vec<_> = points.iter().map(|p| p.1).collect();
    ys.sort_unstable_by(f64::total_cmp);
    ys.dedup();
    let mut queries = Vec::new();
    let mut events = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (gap_index, pair) in row.windows(2).enumerate() {
            let (a, b) = (&pair[0], &pair[1]);
            let (left, right) = if a.layout.x0 <= b.layout.x0 {
                (a, b)
            } else {
                (b, a)
            };
            if left.layout.x1 >= right.layout.x0 {
                continue;
            }
            let em = a.size.unwrap_or(12.0 * point).max(point);
            let low = ys.partition_point(|&y| y < a.origin.1 - em * 0.6);
            let high = ys.partition_point(|&y| y <= a.origin.1 + em * 0.6);
            let id = queries.len();
            let baseline = ys.partition_point(|&y| y <= a.origin.1);
            queries.push((row_index, gap_index, low, high, baseline, 0isize));
            // F(right, exclusive) - F(left, inclusive) counts an open x gap.
            events.push((left.layout.x1, true, id, -1isize));
            events.push((right.layout.x0, false, id, 1isize));
        }
    }
    events.sort_unstable_by(|a, b| {
        // Signed zeros denote the same geometric boundary. Exclusive events
        // must precede inclusive events there, irrespective of their bit sign.
        if a.0 == b.0 {
            a.1.cmp(&b.1)
        } else {
            a.0.total_cmp(&b.0)
        }
    });
    let mut origins = vec![0isize; ys.len() + 1];
    let mut coverage = vec![0isize; ys.len() + 1];
    let mut at = 0;
    for (x, inclusive, id, sign) in events {
        while at < points.len() && (points[at].0 < x || inclusive && points[at].0 == x) {
            let (_, y, radius) = points[at];
            add(&mut origins, ys.partition_point(|&value| value < y), 1);
            // Either font can be the larger base. Counting both origin-near
            // and interval-covering points uses max(em_a, em_b), not their sum.
            add(
                &mut coverage,
                ys.partition_point(|&value| value < y - radius),
                1,
            );
            add(
                &mut coverage,
                ys.partition_point(|&value| value <= y + radius),
                -1,
            );
            at += 1;
        }
        let (_, _, low, high, baseline, count) = &mut queries[id];
        *count += sign
            * (prefix(&origins, *high) - prefix(&origins, *low) + prefix(&coverage, *baseline));
    }
    for (row, gap, _, _, _, count) in queries {
        result[row][gap] = count > 0;
    }
    result
}

fn add(tree: &mut [isize], zero_based: usize, delta: isize) {
    let mut index = zero_based + 1;
    while index < tree.len() {
        tree[index] += delta;
        index += index & index.wrapping_neg();
    }
}

fn prefix(tree: &[isize], mut end: usize) -> isize {
    let mut sum = 0;
    while end > 0 {
        sum += tree[end];
        end &= end - 1;
    }
    sum
}

struct Line {
    y: f64,
    em: f64,
    angle: f64,
    left: f64,
    right: f64,
}

// Finite origins are validated before this module. This monotonic key gives
// the baseline index f64's total order without rounding physical coordinates.
fn key(y: f64) -> u64 {
    let bits = y.to_bits();
    if bits >> 63 == 0 {
        bits ^ (1 << 63)
    } else {
        !bits
    }
}

pub(super) fn sort(runs: &mut [ExtractedRun], point: f64) {
    let mut indices: Vec<_> = (0..runs.len()).collect();
    let em = |r: &ExtractedRun| r.size.unwrap_or(12.0 * point).max(point);
    // Main text establishes the baseline before its smaller scripts. Keep
    // equal-size distinct lines separate even when their leading is narrow.
    indices.sort_unstable_by(|&a, &b| {
        runs[a]
            .source_text_region
            .cmp(&runs[b].source_text_region)
            .then_with(|| em(&runs[b]).total_cmp(&em(&runs[a])))
            .then_with(|| runs[b].origin.1.total_cmp(&runs[a].origin.1))
            .then_with(|| runs[a].layout.x0.total_cmp(&runs[b].layout.x0))
    });
    let mut lines: Vec<Line> = Vec::new();
    let mut baselines: BTreeMap<u64, usize> = BTreeMap::new();
    for i in indices {
        let run = &runs[i];
        if run.vertical || run.angle.rem_euclid(180.0).abs() > 0.1 {
            continue;
        }
        let y = run.origin.1;
        let run_em = em(run);
        // Long/word-separated text is a separate line, not a script on a
        // nearby heading. Typical local indices and exponents are short.
        let script_text =
            run.text.chars().count() <= 8 && !run.text.chars().any(char::is_whitespace);
        let coordinate = key(y);
        // Bound candidate work on hostile/sparse layouts. Four neighboring
        // baselines in each direction cover local scripts without a page scan.
        let candidates = baselines.range(..=coordinate).rev().take(4).chain(
            baselines
                .range((
                    std::ops::Bound::Excluded(coordinate),
                    std::ops::Bound::Unbounded,
                ))
                .take(4),
        );
        let line = candidates
            .filter_map(|(_, &id)| {
                let line = &lines[id];
                let dy = (y - line.y).abs();
                let same_baseline = dy <= (line.em * 0.1).max(0.25 * point);
                let horizontal_gap = (line.left - run.layout.x1).max(run.layout.x0 - line.right);
                let script = script_text
                    && run_em <= line.em * 0.85
                    && run_em >= line.em * 0.4
                    && dy <= line.em * 0.6
                    && horizontal_gap <= line.em;
                // A source replacement may own vectors and have no glyph
                // baseline. Place an adjacent, line-height region using its
                // real rectangle, without changing its exported geometry.
                // Interleaving remains explicitly reviewable at page level.
                let region = run.source_text_region
                    && run.bounds.y0 <= line.y
                    && line.y <= run.bounds.y1
                    && (run.bounds.y1 - run.bounds.y0) <= line.em * 1.5
                    && horizontal_gap <= line.em;
                ((same_baseline || script || region) && (run.angle - line.angle).abs() < 0.1)
                    .then_some((id, dy))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0);
        let baseline = if let Some(id) = line {
            let line = &mut lines[id];
            line.left = line.left.min(run.layout.x0);
            line.right = line.right.max(run.layout.x1);
            line.y
        } else {
            baselines.insert(coordinate, lines.len());
            lines.push(Line {
                y,
                em: run_em,
                angle: run.angle,
                left: run.layout.x0,
                right: run.layout.x1,
            });
            y
        };
        // Only the internal ordering coordinate changes. Exported ink bounds,
        // character spans, font sizes and SDK uncertainty remain untouched.
        runs[i].layout.y1 = baseline;
    }
    runs.sort_by(|a, b| {
        b.layout
            .y1
            .total_cmp(&a.layout.y1)
            .then_with(|| a.layout.x0.total_cmp(&b.layout.x0))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bounds;
    use proptest::prelude::*;

    fn run(x: f64, y: f64, size: f64) -> ExtractedRun {
        let bounds = Bounds {
            x0: x,
            y0: y,
            x1: x,
            y1: y,
        };
        ExtractedRun {
            source_text_region: false,
            source_text_anchor: false,
            source_regions: Vec::new(),
            tagged_cell: None,
            outside_mark: None,
            text: "x".into(),
            sdk_unicode_mapping_errors: 0,
            bounds,
            size: Some(size),
            spans: vec![(x, x)],
            origin: (x, y),
            angle: 0.,
            vertical: false,
            layout: bounds,
        }
    }

    #[test]
    fn signed_zero_boundaries_exclude_both_endpoints() {
        let rows = vec![
            vec![run(-0., 0., 10.), run(1., 0., 10.)],
            vec![run(-1., 0., 10.), run(0., 0., 10.)],
        ];
        assert_eq!(barriers(&rows, 1.), vec![vec![false], vec![false]]);
    }

    #[test]
    fn source_region_sort_uses_adjacent_ink_without_inventing_a_baseline() {
        let mut text = run(20.0, 100.0, 12.0);
        text.text = "This is the".into();
        text.bounds = crate::Bounds {
            x0: 20.0,
            y0: 99.0,
            x1: 70.0,
            y1: 109.0,
        };
        text.layout = text.bounds;
        let mut region = run(73.0, 108.0, 12.0);
        region.source_text_region = true;
        region.text = "paragraph.".into();
        region.bounds = crate::Bounds {
            x0: 73.0,
            y0: 97.0,
            x1: 120.0,
            y1: 108.0,
        };
        region.layout = region.bounds;
        let original_bounds = region.bounds;
        let original_origin = region.origin;
        let original_spans = region.spans.clone();
        let mut runs = vec![region, text];
        sort(&mut runs, 1.0);
        assert_eq!(runs[0].text, "This is the");
        assert_eq!(runs[1].text, "paragraph.");
        assert_eq!(runs[1].bounds, original_bounds);
        assert_eq!(runs[1].origin, original_origin);
        assert_eq!(runs[1].spans, original_spans);
    }

    proptest! {
        #[test]
        fn sweep_matches_independent_rectangle_and_font_distance_checks(
            input in prop::collection::vec((-500i16..500, -500i16..500, 3u8..31, 1u8..41), 0..24)
        ) {
            let rows: Vec<_> = input.into_iter().map(|(x,y,size,gap)| {
                let mut row = vec![run(x.into(), y.into(), size.into()), run(f64::from(x)+f64::from(gap), y.into(), size.into())];
                if x % 2 == 0 { row.reverse(); }
                row
            }).collect();
            let actual = barriers(&rows, 1.);
            for (i, row) in rows.iter().enumerate() {
                let left = row[0].origin.0.min(row[1].origin.0);
                let right = row[0].origin.0.max(row[1].origin.0);
                let expected = rows.iter().flatten().any(|p| p.origin.0 > left && p.origin.0 < right
                    && (p.origin.1 - row[0].origin.1).abs() <= p.size.unwrap().max(row[0].size.unwrap()) * 0.6);
                prop_assert_eq!(actual[i][0], expected);
            }
        }
    }
}
