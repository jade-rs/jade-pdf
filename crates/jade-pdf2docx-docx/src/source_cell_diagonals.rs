//! 端点が同じ空セルの対向角に対応した原本の黒い実線だけをWordの対角罫線へ渡す。
use super::*;
use jade_pdf2docx_parser::ParsedSourceDiagonal;

/// 原本の描画順を保ちながら、ページ内の上端座標で検索できる線分。
struct Candidate<'a> {
    /// 端点・向き・線幅を照合する原本線分。
    line: &'a ParsedSourceDiagonal,
    /// PDF座標で上側にある端点のY。
    upper_y: f32,
    /// 角が重なる候補間の優先順。
    paint_order: usize,
}

/// ページ内の斜線を上端順で索引化し、子表を含む全表で再利用する。
pub(super) fn apply(parsed: &ParsedDocument, tables: &mut [PositionedTable]) {
    let mut pages: BTreeMap<usize, Vec<Candidate<'_>>> = BTreeMap::new();
    for (paint_order, line) in parsed.source_diagonals.iter().enumerate() {
        if line.start[1].is_finite() && line.end[1].is_finite() {
            pages.entry(line.page).or_default().push(Candidate {
                line,
                upper_y: line.start[1].max(line.end[1]),
                paint_order,
            });
        }
    }
    if pages.is_empty() {
        return;
    }
    for candidates in pages.values_mut() {
        candidates.sort_unstable_by(|a, b| a.upper_y.total_cmp(&b.upper_y));
    }
    apply_tables(&pages, tables);
}

/// 表の原本row/column寸法を使い、結合セルも一つの所有領域として照合する。
fn apply_tables(pages: &BTreeMap<usize, Vec<Candidate<'_>>>, tables: &mut [PositionedTable]) {
    for table in tables {
        apply_tables(pages, &mut table.nested_tables);
        let Some(candidates) = pages.get(&table.page) else {
            continue;
        };
        if !table.nested_tables.is_empty()
            || !table.source_cell_runs.is_empty()
            || !table.native_cells.iter().flatten().any(Option::is_some)
        {
            continue;
        }
        let mut model = table
            .docx_table
            .clone()
            .unwrap_or_else(|| docx_table_model_from_ocr_table(&table.table));
        if model.rows.len() != table.row_heights_twips.len() || table.column_widths_twips.is_empty()
        {
            continue;
        }
        let Some(ys) = row_edges(table) else {
            continue;
        };
        let xs = if let Some(grid) = &table.source_rule_grid
            && grid.xs.len() == table.column_widths_twips.len() + 1
        {
            grid.xs.clone()
        } else {
            let mut xs = vec![table.bounds.x0];
            for w in &table.column_widths_twips {
                xs.push(xs.last().copied().unwrap() + *w as f32 / 20.);
            }
            xs
        };
        if !xs.iter().all(|x| x.is_finite())
            || (xs.last().copied().unwrap() - table.bounds.x1).abs() > 1.
        {
            continue;
        }
        let mut changed = false;
        for ri in 0..model.rows.len() {
            let mut col = 0_usize;
            for ci in 0..model.rows[ri].len() {
                let cell = &model.rows[ri][ci];
                let span = cell.grid_span.max(1);
                let endcol = col.saturating_add(span);
                if endcol >= xs.len() {
                    break;
                }
                if !cell.text.trim().is_empty()
                    || cell.vertical_merge == Some(DocxVerticalMerge::Continue)
                {
                    col = endcol;
                    continue;
                }
                let mut last = ri;
                if cell.vertical_merge == Some(DocxVerticalMerge::Restart) {
                    for row in model.rows.iter().skip(ri + 1) {
                        let mut offset = 0;
                        let next = row.iter().find(|c| {
                            let current = offset;
                            offset += c.grid_span.max(1);
                            current == col
                        });
                        if next.is_some_and(|c| {
                            c.grid_span.max(1) == span
                                && c.vertical_merge == Some(DocxVerticalMerge::Continue)
                                && c.text.trim().is_empty()
                        }) {
                            last += 1;
                        } else {
                            break;
                        }
                    }
                }
                let b = ParsedBounds {
                    x0: xs[col],
                    x1: xs[endcol],
                    y0: ys[last + 1],
                    y1: ys[ri],
                };
                let mut borders = cell.borders.unwrap_or_else(DocxCellBorders::all);
                if borders.top && borders.bottom && borders.left && borders.right {
                    changed |= apply_cell(candidates, b, &mut borders);
                    model.rows[ri][ci].borders = Some(borders);
                }
                col = endcol;
            }
        }
        if changed {
            table.docx_table = Some(model);
        }
    }
}

/// 上端が対応する範囲だけを確認し、方向ごとに原本で最後に描いた線を選ぶ。
fn apply_cell(
    candidates: &[Candidate<'_>],
    b: ParsedBounds,
    borders: &mut DocxCellBorders,
) -> bool {
    let start =
        candidates.partition_point(|candidate| candidate.upper_y - b.y1 < -CORNER_TOLERANCE);
    let candidates = &candidates[start..];
    let end = candidates.partition_point(|candidate| candidate.upper_y - b.y1 <= CORNER_TOLERANCE);
    let mut latest: [Option<&Candidate<'_>>; 2] = [None; 2];
    for candidate in &candidates[..end] {
        if let Some(tl2br) = direction(candidate.line, b) {
            let last = &mut latest[usize::from(!tl2br)];
            if last.is_none_or(|last| candidate.paint_order > last.paint_order) {
                *last = Some(candidate);
            }
        }
    }
    if let Some(candidate) = latest[0] {
        borders.diagonal_tl2br = Some(table_border_size_for_pdf_rule(candidate.line.stroke_width));
    }
    if let Some(candidate) = latest[1] {
        borders.diagonal_tr2bl = Some(table_border_size_for_pdf_rule(candidate.line.stroke_width));
    }
    latest.iter().any(Option::is_some)
}

/// セルの角と斜線の端点の照合許容値（PDF point）。
const CORNER_TOLERANCE: f32 = 1.;

/// 各元行のtwip累積から順序付き境界を作り、表高との一致も確認する。
fn row_edges(table: &PositionedTable) -> Option<Vec<f32>> {
    if let Some(grid) = &table.source_rule_grid
        && grid.ys.len() == table.row_heights_twips.len() + 1
    {
        // Wordの薄罫線補正を原本の空セルの高さへ加えない。
        return Some(grid.ys.iter().copied().rev().collect());
    }
    let mut ys = vec![table.bounds.y1];
    for h in &table.row_heights_twips {
        if !h.is_finite() || *h <= 0. {
            return None;
        }
        ys.push(ys.last().copied()? - h / 20.);
    }
    (ys.iter().all(|y| y.is_finite()) && (ys.last().copied()? - table.bounds.y0).abs() <= 1.)
        .then_some(ys)
}

/// 方向に依存しない端点照合で、セルの対向角に対応する線だけを認める。
fn direction(line: &ParsedSourceDiagonal, b: ParsedBounds) -> Option<bool> {
    if !line
        .start
        .iter()
        .chain(&line.end)
        .chain([&line.stroke_width])
        .all(|v| v.is_finite())
        || line.stroke_width <= 0.
        || ![b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite())
        || b.x1 - b.x0 <= 2.
        || b.y1 - b.y0 <= 2.
    {
        return None;
    }
    let close = |p: [f32; 2], q: [f32; 2]| {
        (p[0] - q[0]).abs() <= CORNER_TOLERANCE && (p[1] - q[1]).abs() <= CORNER_TOLERANCE
    };
    for (tl2br, a, c) in [
        (true, [b.x0, b.y1], [b.x1, b.y0]),
        (false, [b.x1, b.y1], [b.x0, b.y0]),
    ] {
        if (close(line.start, a) && close(line.end, c))
            || (close(line.start, c) && close(line.end, a))
        {
            return Some(tl2br);
        }
    }
    None
}
