//! 物理gridを確認した単純な表で、原本の各辺全体を覆う線幅だけをセル罫線へ渡す。
use super::*;

#[path = "source_cell_rule_pitch.rs"]
mod pitch;

/// 同じ軸座標にある原本線分を、表のセル数に比例する全graphics再走査なしで参照する。
struct Rules<'a> {
    /// 水平線の固定Yと、原本の区間・太さ。
    horizontal: BTreeMap<u32, Vec<&'a ParsedGraphicElement>>,
    /// 垂直線の固定Xと、原本の区間・太さ。
    vertical: BTreeMap<u32, Vec<&'a ParsedGraphicElement>>,
}

/// 同一辺のWord幅が一意な線分群。区間の空隙と角で接する直交線を別に照合する。
struct Edge {
    /// 角のink接続で過大な延長をしない、候補群の最小原本幅。
    width: f32,
    /// 既存Word線幅単位へ量子化した値。
    size: usize,
    /// 原本の実在区間。小さい空隙も推定用の許容値で結合しない。
    intervals: Vec<(f32, f32)>,
}

/// 浮動小数点の等値と同じく、正負のゼロだけを同じキーにする。
fn key(value: f32) -> u32 {
    if value == 0. { 0 } else { value.to_bits() }
}

impl<'a> Rules<'a> {
    /// 一ページの有限な軸平行線だけを索引化し、線幅不明の候補は辺の照合時に拒否する。
    fn new(parsed: &'a ParsedDocument, page: usize, grid: &fractional_table_rules::Grid) -> Self {
        let mut out = Self {
            horizontal: grid.ys.iter().map(|y| (key(*y), Vec::new())).collect(),
            vertical: grid.xs.iter().map(|x| (key(*x), Vec::new())).collect(),
        };
        for graphic in parsed.graphics.iter().filter(|g| g.page == page) {
            let b = graphic.bounds;
            if ![b.x0, b.x1, b.y0, b.y1].iter().all(|v| v.is_finite()) {
                continue;
            }
            if b.x0 < b.x1
                && b.y0 == b.y1
                && b.x1 > grid.xs[0]
                && b.x0 < grid.xs[grid.xs.len() - 1]
                && let Some(axis) = out.horizontal.get_mut(&key(b.y0))
            {
                axis.push(graphic);
            } else if b.y0 < b.y1
                && b.x0 == b.x1
                && b.y1 > grid.ys[0]
                && b.y0 < grid.ys[grid.ys.len() - 1]
                && let Some(axis) = out.vertical.get_mut(&key(b.x0))
            {
                axis.push(graphic);
            }
        }
        out
    }

    /// 辺内の全候補が同じWord幅へ対応する場合だけ、元の区間を返す。
    fn edge(&self, vertical: bool, fixed: f32, low: f32, high: f32) -> Option<Edge> {
        let map = if vertical {
            &self.vertical
        } else {
            &self.horizontal
        };
        let mut edge: Option<Edge> = None;
        for graphic in map.get(&key(fixed))? {
            let b = graphic.bounds;
            let (start, end) = if vertical { (b.y0, b.y1) } else { (b.x0, b.x1) };
            if end <= low || start >= high {
                continue;
            }
            let width = graphic.stroke_width;
            if !width.is_finite() || width <= 0. || width > 12. {
                return None;
            }
            let size = table_border_size_for_pdf_rule(width);
            if let Some(edge) = &mut edge {
                if edge.size != size {
                    return None;
                }
                edge.width = edge.width.min(width);
                edge.intervals.push((start, end));
            } else {
                edge = Some(Edge {
                    width,
                    size,
                    intervals: vec![(start, end)],
                });
            }
        }
        let mut edge = edge?;
        edge.intervals
            .sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        Some(edge)
    }
}

impl Edge {
    /// 角の中心を実在する直交線が通る場合だけ、その原本ink半幅を端点へ許す。
    fn corner_padding(&self, position: f32, epsilon: f32) -> f32 {
        if self
            .intervals
            .iter()
            .any(|(a, b)| *a <= position + epsilon && *b >= position - epsilon)
        {
            self.width / 2.
        } else {
            0.
        }
    }

    /// 端の接合ink以外の空隙を許さず、辺の必要範囲を連続する原本線分で確認する。
    fn covers(&self, low: f32, high: f32, lower: f32, upper: f32) -> bool {
        let epsilon = 0.001_f32.min((high - low) / 4.);
        let mut reached = low + lower;
        let target = high - upper;
        if reached >= target {
            return false;
        }
        for (start, end) in &self.intervals {
            if *start > reached + epsilon {
                return false;
            }
            reached = reached.max(*end);
            if reached >= target - epsilon {
                return true;
            }
        }
        false
    }
}

/// 所有・font・物理gridが確認できる非結合セルだけ、最大幅から原本の各辺幅へ精密化する。
pub(super) fn apply(parsed: &ParsedDocument, tables: &mut [PositionedTable]) {
    for table in tables {
        apply(parsed, &mut table.nested_tables);
        let Some(grid) = &table.source_rule_grid else {
            continue;
        };
        if !table.nested_tables.is_empty() || grid.ys.len() != table.table.rows.len() + 1 {
            continue;
        }
        let default = table_border_size_for_pdf_rule(table.stroke_width);
        if !parsed.graphics.iter().any(|g| {
            g.page == table.page
                && g.stroke_width.is_finite()
                && g.stroke_width > 0.
                && table_border_size_for_pdf_rule(g.stroke_width) != default
        }) {
            continue;
        }
        let mut model = table
            .docx_table
            .clone()
            .unwrap_or_else(|| docx_table_model_from_ocr_table(&table.table));
        let rows = grid.ys.len() - 1;
        let cols = grid.xs.len() - 1;
        if model.rows.len() != rows
            || model.rows.iter().any(|row| {
                row.len() != cols
                    || row.iter().any(|cell| {
                        cell.grid_span != 1
                            || cell.vertical_merge.is_some()
                            || cell
                                .borders
                                .is_some_and(|b| !(b.top && b.left && b.bottom && b.right))
                    })
            })
        {
            continue;
        }
        let rules = Rules::new(parsed, table.page, grid);
        let mut changed = false;
        for (ri, row) in model.rows.iter_mut().enumerate() {
            for (ci, cell) in row.iter_mut().enumerate() {
                let (left, right, bottom, top) = (
                    grid.xs[ci],
                    grid.xs[ci + 1],
                    grid.ys[rows - ri - 1],
                    grid.ys[rows - ri],
                );
                let edges = [
                    rules.edge(false, top, left, right),
                    rules.edge(true, left, bottom, top),
                    rules.edge(false, bottom, left, right),
                    rules.edge(true, right, bottom, top),
                ];
                let mut borders = cell.borders.unwrap_or_else(DocxCellBorders::all);
                for (index, edge) in edges.iter().enumerate() {
                    let Some(edge) = edge else {
                        continue;
                    };
                    let (low, high, lower, upper, position) = if index == 0 || index == 2 {
                        (left, right, 1, 3, if index == 0 { top } else { bottom })
                    } else {
                        (bottom, top, 2, 0, if index == 1 { left } else { right })
                    };
                    let epsilon = 0.001_f32.min((high - low) / 4.);
                    let padding = |i: usize| {
                        edges[i]
                            .as_ref()
                            .map_or(0., |e| e.corner_padding(position, epsilon))
                    };
                    if edge.size != default
                        && edge.covers(low, high, padding(lower), padding(upper))
                    {
                        borders.source_side_sizes[index] = Some(edge.size);
                        changed = true;
                    }
                }
                cell.borders = Some(borders);
            }
        }
        if changed {
            pitch::apply(table, &mut model);
            if model.rows.iter().flatten().any(|cell| {
                cell.borders
                    .is_some_and(|b| b.source_side_sizes.iter().any(Option::is_some))
            }) {
                table.docx_table = Some(model);
            }
        }
    }
}
