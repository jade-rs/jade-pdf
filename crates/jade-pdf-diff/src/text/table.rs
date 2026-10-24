//! Keep a completely associated tagged table together in geometric page flow,
//! then emit geometrically ordered rows with declared cell membership/order.
//! Cell order must agree with a consistent horizontal direction. Row bands may
//! overlap for spanning cells only where their horizontal positions are disjoint.
//! This avoids trusting producer row lists that disagree with the painted page.
//! Cell contents retain
//! geometric line order and every original scalar, span and diagnostic.
use super::{order, ExtractedRun};
use crate::{tagged_table::Cell, Bounds};
use std::collections::{BTreeMap, HashSet};

fn inside(b: Bounds, r: &ExtractedRun) -> bool {
    b.x0 <= r.origin.0 && r.origin.0 <= b.x1 && b.y0 <= r.origin.1 && r.origin.1 <= b.y1
}

/// One stale cell reference can be repaired only against at least two complete,
/// consistent reference rows. The orphan must be one page-scoped structural
/// MCID, fit the unique declared vacancy, and have no content elsewhere.
/// The caller always reports this inferred membership as requiring review.
fn recover_one(
    runs: &mut [ExtractedRun],
    table: u32,
    bounds: Bounds,
    boxes: &BTreeMap<u32, Bounds>,
    declarations: &[crate::tagged_table::Declaration],
    point: f64,
    work: &mut usize,
) -> Option<()> {
    charge(
        work,
        runs.len()
            .saturating_mul(4)
            .saturating_add(declarations.len()),
    )?;
    let declared: Vec<_> = declarations
        .iter()
        .filter(|d| d.cell.table == table)
        .collect();
    let missing: Vec<_> = declared.iter().filter(|d| !d.observed).collect();
    if missing.len() != 1 {
        return None;
    }
    let missing = **missing[0];
    let span = missing.spans?;
    if span.rows == 0 || (span.rows == 1 && span.columns == 1) {
        return None;
    }
    let mut cells = BTreeMap::<Cell, Bounds>::new();
    for run in runs.iter() {
        if let Some(cell) = run.tagged_cell.filter(|c| c.table == table) {
            if cell.ordinal == u32::MAX {
                return None;
            }
            cells
                .entry(cell)
                .and_modify(|b| *b = union(*b, run.layout))
                .or_insert(run.layout);
        }
    }
    if cells.contains_key(&missing.cell) {
        return None;
    }
    let mut rows = BTreeMap::<u32, Vec<_>>::new();
    for declaration in declared {
        rows.entry(declaration.cell.row)
            .or_default()
            .push(declaration);
    }
    for row in rows.values_mut() {
        row.sort_by_key(|d| d.cell.ordinal);
    }
    let reference: Vec<_> = rows
        .values()
        .filter(|row| {
            row.iter().enumerate().all(|(i, d)| {
                d.observed
                    && cells.contains_key(&d.cell)
                    && d.cell.ordinal == i as u32
                    && d.spans.is_some_and(|s| s.columns == 1 && s.rows == 1)
            })
        })
        .collect();
    let columns = reference.iter().map(|r| r.len()).max()?;
    if !(2..=64).contains(&columns) {
        return None;
    }
    let reference: Vec<_> = reference
        .into_iter()
        .filter(|r| r.len() == columns)
        .collect();
    if reference.len() < 2 {
        return None;
    }
    let mut column_boxes = Vec::new();
    for index in 0..columns {
        column_boxes.push(
            reference
                .iter()
                .map(|row| cells[&row[index].cell])
                .reduce(union)?,
        );
    }
    let ltr = column_boxes
        .windows(2)
        .all(|p| p[0].x1 <= p[1].x0 + point * 0.5);
    let rtl = column_boxes
        .windows(2)
        .all(|p| p[1].x1 <= p[0].x0 + point * 0.5);
    if ltr == rtl {
        return None;
    }
    let region = |start: usize, count: usize| {
        let end = start + count - 1;
        if ltr {
            (
                if start == 0 {
                    f64::NEG_INFINITY
                } else {
                    (column_boxes[start - 1].x1 + column_boxes[start].x0) / 2.0
                },
                if end + 1 == columns {
                    f64::INFINITY
                } else {
                    (column_boxes[end].x1 + column_boxes[end + 1].x0) / 2.0
                },
            )
        } else {
            (
                if end + 1 == columns {
                    f64::NEG_INFINITY
                } else {
                    (column_boxes[end + 1].x1 + column_boxes[end].x0) / 2.0
                },
                if start == 0 {
                    f64::INFINITY
                } else {
                    (column_boxes[start].x1 + column_boxes[start - 1].x0) / 2.0
                },
            )
        }
    };
    let row = rows.get(&missing.cell.row)?;
    let mut offset = 0usize;
    let mut vacancy = None;
    for (ordinal, d) in row.iter().enumerate() {
        let width = d.spans?.columns as usize;
        if width == 0 || d.cell.ordinal != ordinal as u32 || offset.checked_add(width)? > columns {
            return None;
        }
        let r = region(offset, width);
        if d.cell == missing.cell {
            vacancy = Some(r);
        } else {
            let b = cells.get(&d.cell)?;
            let center = (b.x0 + b.x1) / 2.0;
            if center < r.0 - point * 0.5 || center > r.1 + point * 0.5 {
                return None;
            }
        }
        offset += width;
    }
    if offset != columns {
        return None;
    }
    let (left, right) = vacancy?;
    let mut row_boxes = BTreeMap::<u32, Bounds>::new();
    for (cell, b) in &cells {
        row_boxes
            .entry(cell.row)
            .and_modify(|a| *a = union(*a, *b))
            .or_insert(*b);
    }
    let mut order: Vec<_> = row_boxes.into_iter().collect();
    order.sort_by(|a, b| b.1.y1.total_cmp(&a.1.y1).then(a.0.cmp(&b.0)));
    let rank = order.iter().position(|(row, _)| *row == missing.cell.row)?;
    if rank + span.rows as usize > order.len() {
        return None;
    }
    let top = order[rank].1.y1;
    let bottom = order
        .get(rank + span.rows as usize)
        .map_or(bounds.y0, |(_, b)| b.y1);
    let mut orphan = None;
    for run in runs
        .iter()
        .filter(|r| r.tagged_cell.is_none_or(|c| c.table != table) && inside(bounds, r))
    {
        if run.tagged_cell.is_some() {
            return None;
        }
        let id = run.outside_mark?;
        if orphan.is_some_and(|old| old != id) {
            return None;
        }
        orphan = Some(id);
    }
    let id = orphan?;
    for run in runs.iter().filter(|r| r.outside_mark == Some(id)) {
        if run.vertical
            || run
                .angle
                .rem_euclid(360.0)
                .min((-run.angle).rem_euclid(360.0))
                > 0.1
            || run.layout.x0 < left - point * 0.5
            || run.layout.x1 > right + point * 0.5
            || run.layout.y1 > top + point * 0.5
            || run.layout.y0 < bottom - point * 0.5
        {
            return None;
        }
        for (&other, b) in boxes {
            charge(work, 1)?;
            if other != table && inside(*b, run) {
                return None;
            }
        }
    }
    for run in runs.iter_mut().filter(|r| r.outside_mark == Some(id)) {
        run.tagged_cell = Some(missing.cell);
    }
    Some(())
}

fn union(a: Bounds, b: Bounds) -> Bounds {
    Bounds {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

fn charge(work: &mut usize, amount: usize) -> Option<()> {
    *work = work.checked_sub(amount)?;
    Some(())
}

fn row_order(runs: &[&ExtractedRun], point: f64, work: &mut usize) -> Option<BTreeMap<u32, u32>> {
    charge(work, runs.len())?;
    let mut cells = BTreeMap::<(u32, u32), Bounds>::new();
    for r in runs {
        let cell = r.tagged_cell?;
        cells
            .entry((cell.row, cell.ordinal))
            .and_modify(|b| *b = union(*b, r.layout))
            .or_insert(r.layout);
    }
    struct Row {
        id: u32,
        bounds: Bounds,
        cells: Vec<Bounds>,
    }
    let mut rows = BTreeMap::<u32, Row>::new();
    for ((row, _), bounds) in cells {
        let entry = rows.entry(row).or_insert(Row {
            id: row,
            bounds,
            cells: Vec::new(),
        });
        entry.bounds = union(entry.bounds, bounds);
        entry.cells.push(bounds);
    }
    // Ordinal is local to a row, not a global column number: empty cells,
    // merged headers and row spans can change the number of cells per row.
    let mut left_to_right = true;
    let mut right_to_left = true;
    for row in rows.values() {
        for pair in row.cells.windows(2) {
            charge(work, 1)?;
            left_to_right &= pair[0].x1 <= pair[1].x0 + point * 0.5;
            right_to_left &= pair[1].x1 <= pair[0].x0 + point * 0.5;
        }
    }
    if !left_to_right && !right_to_left {
        return None;
    }
    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by(|a, b| b.bounds.y1.total_cmp(&a.bounds.y1).then(a.id.cmp(&b.id)));
    if rows
        .windows(2)
        .any(|p| p[0].bounds.y1 - p[1].bounds.y1 <= point * 0.1)
    {
        return None;
    }
    // A spanning cell can cross the next row's vertical band. Check only
    // horizontally shared positions: an earlier row may not continue below a
    // later row in the same column. This also rejects crossed row membership.
    // Disjoint row bands need no cell-pair comparisons. All other comparisons
    // consume the same page-local optional work budget as completeness scans.
    for (index, upper) in rows.iter().enumerate() {
        for lower in &rows[index + 1..] {
            charge(work, 1)?;
            if lower.bounds.y1 <= upper.bounds.y0 + point * 0.5 {
                break;
            }
            for a in &upper.cells {
                for b in &lower.cells {
                    charge(work, 1)?;
                    let shared_x = a.x0.max(b.x0) <= a.x1.min(b.x1) + point * 0.5;
                    if shared_x && b.y1 > a.y0 + point * 0.5 {
                        return None;
                    }
                }
            }
        }
    }
    // Single-column rows still need a common horizontal position.
    if rows.iter().all(|row| row.cells.len() == 1) {
        let left = rows
            .iter()
            .map(|row| row.bounds.x0)
            .fold(f64::NEG_INFINITY, f64::max);
        let right = rows
            .iter()
            .map(|row| row.bounds.x1)
            .fold(f64::INFINITY, f64::min);
        if left > right + point * 0.5 {
            return None;
        }
    }
    Some(
        rows.into_iter()
            .enumerate()
            .map(|(rank, row)| (row.id, rank as u32))
            .collect(),
    )
}

/// Do not let partial tags draw ordinary text into an inferred table. A source
/// run belonging to another/no table inside its origin rectangle invalidates
/// the candidate. Bound these optional completeness checks to two million.
pub(super) fn retain_complete(
    runs: &mut [ExtractedRun],
    point: f64,
    declarations: &[crate::tagged_table::Declaration],
) -> bool {
    let has_tags = runs.iter().any(|r| r.tagged_cell.is_some());
    if runs.iter().filter(|r| r.vertical).count() > runs.len() / 2 {
        for r in runs {
            r.tagged_cell = None;
            r.outside_mark = None;
        }
        return has_tags;
    }
    let mut boxes = BTreeMap::<u32, Bounds>::new();
    let mut invalid = HashSet::new();
    for r in runs.iter() {
        if let Some(cell) = r.tagged_cell {
            boxes
                .entry(cell.table)
                .and_modify(|b| *b = union(*b, r.layout))
                .or_insert(r.layout);
            if cell.ordinal == u32::MAX
                || r.vertical
                || r.angle.rem_euclid(360.0).min((-r.angle).rem_euclid(360.0)) > 0.1
            {
                invalid.insert(cell.table);
            }
        }
    }
    let mut work = 2_000_000usize;
    if charge(
        &mut work,
        runs.len().saturating_mul(boxes.len()).saturating_mul(3),
    )
    .is_none()
    {
        for r in runs {
            r.tagged_cell = None;
            r.outside_mark = None;
        }
        return has_tags;
    }
    let mut ranks = BTreeMap::new();
    let mut inferred = false;
    for (&table, b) in &boxes {
        let foreign = runs
            .iter()
            .any(|r| r.tagged_cell.is_none_or(|c| c.table != table) && inside(*b, r));
        if foreign
            && !invalid.contains(&table)
            && recover_one(runs, table, *b, &boxes, declarations, point, &mut work).is_some()
        {
            inferred = true;
        }
        let members: Vec<_> = runs
            .iter()
            .filter(|r| r.tagged_cell.is_some_and(|c| c.table == table))
            .collect();
        let order = row_order(&members, point, &mut work);
        if order.is_none()
            || runs
                .iter()
                .any(|r| r.tagged_cell.is_none_or(|c| c.table != table) && inside(*b, r))
        {
            invalid.insert(table);
        } else if let Some(order) = order {
            ranks.insert(table, order);
        }
    }
    for r in runs {
        r.outside_mark = None;
        if let Some(cell) = &mut r.tagged_cell {
            if invalid.contains(&cell.table) {
                r.tagged_cell = None;
            } else {
                cell.row = ranks[&cell.table][&cell.row];
            }
        }
    }
    inferred || !invalid.is_empty()
}

pub(super) fn sort(runs: Vec<ExtractedRun>, point: f64) -> Vec<ExtractedRun> {
    if !runs.iter().any(|r| r.tagged_cell.is_some()) {
        let mut runs = runs;
        order::sort_runs(&mut runs, point);
        return runs;
    }
    let count = runs.len();
    let mut tables = BTreeMap::<u32, Vec<ExtractedRun>>::new();
    let mut units = Vec::new();
    for run in runs {
        if let Some(cell) = run.tagged_cell {
            tables.entry(cell.table).or_default().push(run);
        } else {
            units.push(run);
        }
    }
    for (&table, members) in &mut tables {
        members.sort_by_key(|r| r.tagged_cell.map(|c| (c.row, c.ordinal)));
        let mut start = 0;
        while start < members.len() {
            let cell = members[start].tagged_cell;
            let end = start + members[start..].partition_point(|r| r.tagged_cell == cell);
            order::sort_runs(&mut members[start..end], point);
            start = end;
        }
        let first = &members[0];
        let bounds = members
            .iter()
            .skip(1)
            .fold(first.bounds, |b, r| union(b, r.bounds));
        let layout = members
            .iter()
            .skip(1)
            .fold(first.layout, |b, r| union(b, r.layout));
        // This empty sort unit represents the table's position only. It is
        // replaced with all original members below, never exported as text.
        // A whitespace sentinel prevents inline-script attachment heuristics.
        units.push(ExtractedRun {
            source_text_region: false,
            source_text_anchor: false,
            source_regions: Vec::new(),
            text: "\n".into(),
            sdk_unicode_mapping_errors: 0,
            bounds,
            size: first.size,
            spans: Vec::new(),
            origin: (layout.x0, layout.y1),
            angle: 0.0,
            vertical: false,
            layout,
            tagged_cell: Some(Cell {
                table,
                row: u32::MAX,
                ordinal: u32::MAX,
            }),
            outside_mark: None,
        });
    }
    order::sort_runs(&mut units, point);
    let mut result = Vec::with_capacity(count);
    for unit in units {
        if let Some(cell) = unit.tagged_cell {
            result.extend(
                tables
                    .remove(&cell.table)
                    .expect("Table sort unit retains all original members"),
            );
        } else {
            result.push(unit);
        }
    }
    debug_assert_eq!(result.len(), count);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_row_validation_stops_at_the_shared_optional_budget() {
        let runs: Vec<_> = (0..100)
            .map(|row| {
                let x = f64::from(row) * 2.0;
                let b = Bounds {
                    x0: x,
                    x1: x + 1.0,
                    y0: 0.0,
                    y1: 1000.0 - f64::from(row),
                };
                ExtractedRun {
                    source_text_region: false,
                    source_text_anchor: false,
                    source_regions: Vec::new(),
                    text: "x".into(),
                    sdk_unicode_mapping_errors: 1,
                    bounds: b,
                    size: Some(12.0),
                    spans: vec![(x, x + 1.0)],
                    origin: (x, b.y1),
                    angle: 0.0,
                    vertical: false,
                    layout: b,
                    tagged_cell: Some(Cell {
                        table: 0,
                        row,
                        ordinal: 0,
                    }),
                    outside_mark: None,
                }
            })
            .collect();
        // Every row band overlaps, but the cells are horizontally disjoint.
        // Pair checks must stop before visiting every potential pair.
        let mut work = 1000;
        assert!(row_order(&runs.iter().collect::<Vec<_>>(), 1.0, &mut work).is_none());
        assert_eq!(work, 0);
        assert_eq!(
            runs.iter()
                .map(|r| r.sdk_unicode_mapping_errors)
                .sum::<usize>(),
            100
        );
    }
    #[test]
    fn completeness_budget_falls_back_without_removing_text_or_diagnostics() {
        let mut runs: Vec<_> = (0..20_000)
            .map(|i| {
                let x = f64::from(i);
                let b = Bounds {
                    x0: x,
                    y0: 0.0,
                    x1: x + 1.0,
                    y1: 1.0,
                };
                ExtractedRun {
                    source_text_region: false,
                    source_text_anchor: false,
                    source_regions: Vec::new(),
                    text: "x".into(),
                    sdk_unicode_mapping_errors: 1,
                    bounds: b,
                    size: Some(12.0),
                    spans: vec![(x, x + 1.0)],
                    origin: (x, 0.0),
                    angle: 0.0,
                    vertical: false,
                    layout: b,
                    tagged_cell: Some(Cell {
                        table: i % 101,
                        row: i / 101,
                        ordinal: 0,
                    }),
                    outside_mark: None,
                }
            })
            .collect();
        assert!(retain_complete(&mut runs, 1.0, &[]));
        assert!(runs.iter().all(|r| r.tagged_cell.is_none()));
        let result = sort(runs, 1.0);
        assert_eq!(result.len(), 20_000);
        assert_eq!(result.iter().map(|r| r.text.len()).sum::<usize>(), 20_000);
        assert_eq!(
            result
                .iter()
                .map(|r| r.sdk_unicode_mapping_errors)
                .sum::<usize>(),
            20_000
        );
    }
}
