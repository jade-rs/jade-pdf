//! 閉じた罫線のセルを行方向に並べ、外部の見出しや日付を表の中へ取り込まない。
use super::{order, table, ExtractedRun};
use crate::{ruled_grid::Plan, Bounds};
use std::collections::{BTreeMap, BTreeSet};

/// 二つの矩形を覆う最小の矩形を返す。
fn union(a: Bounds, b: Bounds) -> Bounds {
    Bounds {
        x0: a.x0.min(b.x0),
        x1: a.x1.max(b.x1),
        y0: a.y0.min(b.y0),
        y1: a.y1.max(b.y1),
    }
}

/// 原点だけのゼロ高レイアウトも障害物とし、同じbaselineの外部日付を見落とさない。
fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

/// 外部の文字が同じ帯にあるセルを除き、安全な水平帯ごとに候補を分ける。
fn groups(runs: &[ExtractedRun], grid: &Plan) -> Vec<Vec<usize>> {
    if runs
        .len()
        .saturating_mul(grid.faces.len())
        .saturating_mul(4)
        > 2_000_000
    {
        return Vec::new();
    }
    let mut cells = BTreeMap::<usize, Vec<usize>>::new();
    for (i, run) in runs.iter().enumerate() {
        if let Some(cell) = run.ruled_cell {
            cells.entry(cell).or_default().push(i);
        }
    }
    let mut components = BTreeMap::<usize, Vec<usize>>::new();
    for &cell in cells.keys() {
        components
            .entry(grid.components[cell])
            .or_default()
            .push(cell);
    }
    let mut result = Vec::new();
    for (component, members) in components {
        let bounds = members
            .iter()
            .flat_map(|c| &cells[c])
            .map(|&i| runs[i].layout)
            .reduce(union)
            .unwrap();
        let mut outside: Vec<_> = runs
            .iter()
            .filter(|r| {
                r.ruled_cell.is_none_or(|c| grid.components[c] != component)
                    && overlaps(bounds, r.layout)
            })
            .map(|r| (r.layout.y0, r.layout.y1))
            .collect();
        outside.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut bands: Vec<(f64, f64)> = Vec::new();
        for (low, high) in outside {
            if let Some(last) = bands.last_mut().filter(|last| high >= last.0) {
                last.0 = last.0.min(low);
                last.1 = last.1.max(high);
            } else {
                bands.push((low, high));
            }
        }
        let mut parts = BTreeMap::<usize, Vec<usize>>::new();
        for cell in members {
            let text = cells[&cell]
                .iter()
                .map(|&i| runs[i].layout)
                .reduce(union)
                .unwrap();
            if bands.iter().any(|&(lo, hi)| text.y0 <= hi && lo <= text.y1) {
                continue;
            }
            let slot = bands.partition_point(|&(lo, _)| lo >= text.y1);
            parts.entry(slot).or_default().push(cell);
        }
        for mut part in parts.into_values() {
            // 単一行の段組み枠や一列の囲み段落は、行優先の表だと推定しない。
            let mut rows = BTreeMap::<i64, BTreeSet<i64>>::new();
            for &cell in &part {
                let b = grid.faces[cell].bounds;
                rows.entry((b.y1 * 4096.0).round() as i64)
                    .or_default()
                    .insert((b.x0 * 4096.0).round() as i64);
            }
            if rows.values().filter(|r| r.len() >= 2).count() < 2 {
                continue;
            }
            let indices: BTreeSet<_> = part.iter().flat_map(|c| &cells[c]).copied().collect();
            let bounds = indices
                .iter()
                .map(|&i| runs[i].layout)
                .reduce(union)
                .unwrap();
            if runs.iter().enumerate().any(|(i, r)| {
                (!indices.contains(&i) && overlaps(bounds, r.layout))
                    || (indices.contains(&i)
                        && (r.tagged_cell.is_some()
                            || r.text.chars().any(super::is_rtl)
                            || (!r.vertical && r.angle.rem_euclid(180.0).abs() > 0.1)))
            }) {
                continue;
            }
            part.sort_by(|&a, &b| {
                grid.faces[b]
                    .bounds
                    .y1
                    .total_cmp(&grid.faces[a].bounds.y1)
                    .then_with(|| grid.faces[a].bounds.x0.total_cmp(&grid.faces[b].bounds.x0))
            });
            result.push(part);
        }
    }
    result
}

/// セル内の原文を順序付けた後に表の位置だけをページフローへ戻す。
pub(super) fn sort(runs: Vec<ExtractedRun>, point: f64, grid: &Plan) -> Vec<ExtractedRun> {
    let groups = groups(&runs, grid);
    if groups.is_empty() {
        return table::sort(runs, point);
    }
    let count = runs.len();
    let ownership: BTreeMap<_, _> = groups
        .iter()
        .enumerate()
        .flat_map(|(i, g)| g.iter().map(move |&c| (c, i)))
        .collect();
    let mut members: BTreeMap<usize, BTreeMap<usize, Vec<ExtractedRun>>> = BTreeMap::new();
    let mut units = Vec::new();
    for run in runs {
        if let Some((cell, group)) = run
            .ruled_cell
            .and_then(|c| ownership.get(&c).map(|&g| (c, g)))
        {
            members
                .entry(group)
                .or_default()
                .entry(cell)
                .or_default()
                .push(run);
        } else {
            units.push(run);
        }
    }
    let mut ordered = BTreeMap::new();
    for (i, cells) in groups.iter().enumerate() {
        let mut members = members.remove(&i).unwrap();
        let mut original = Vec::new();
        for cell in cells {
            let mut content = members.remove(cell).unwrap();
            order::sort_runs(&mut content, point);
            original.extend(content);
        }
        let bounds = original.iter().map(|r| r.bounds).reduce(union).unwrap();
        let layout = original.iter().map(|r| r.layout).reduce(union).unwrap();
        // 空白マーカーは表示せず、座標で整列した後に元の全runへ展開する。
        units.push(ExtractedRun {
            advance: Default::default(),
            pending_spaces: Vec::new(),
            text: "\n".into(),
            source_text_region: false,
            source_text_anchor: false,
            source_regions: Vec::new(),
            sdk_unicode_mapping_errors: 0,
            bounds,
            size: original[0].size,
            spans: Vec::new(),
            origin: (layout.x0, layout.y1),
            angle: 0.0,
            vertical: false,
            layout,
            ruled_cell: Some(cells[0]),
            tagged_cell: None,
            outside_mark: None,
        });
        ordered.insert(cells[0], original);
    }
    let mut result = Vec::with_capacity(count);
    for unit in table::sort(units, point) {
        if let Some(original) = unit.ruled_cell.and_then(|c| ordered.remove(&c)) {
            result.extend(original);
        } else {
            result.push(unit);
        }
    }
    debug_assert!(ordered.is_empty());
    debug_assert_eq!(result.len(), count);
    result
}
