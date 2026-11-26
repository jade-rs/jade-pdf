//! 罫線のない本文帯の同期した表示行を、元の辺と文字位置を保つWord行へ分ける。
use super::*;

/// 一つの物理行から、照合できた表示行と同じ数だけ作ったWord行。
struct Split {
    /// 元の列幅と横結合を保つ行モデル。
    rows: Vec<Vec<DocxTableCellModel>>,
    /// 原本の表示行・font・原点に対応するセル。
    cells: Vec<Vec<Option<Cell>>>,
    /// 元の物理行の全高を保存するtwip行高。
    heights: Vec<f32>,
}

/// 原本との対応が完全な罫線表だけを対象に、本文行を局所的に細分する。
pub(super) fn refine(mut table: PositionedTable) -> PositionedTable {
    if !table.nested_tables.is_empty() || !table.source_cell_runs.is_empty() {
        return table;
    }
    let Some(model) = table.docx_table.as_ref() else {
        return table;
    };
    if model.rows.len() != table.native_cells.len()
        || model.rows.len() != table.row_heights_twips.len()
    {
        return table;
    }
    let Some(cols) = table.table.col_count() else {
        return table;
    };
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut heights = Vec::new();
    let mut changed = false;
    for ((row, native), height) in model
        .rows
        .iter()
        .zip(&table.native_cells)
        .zip(&table.row_heights_twips)
    {
        if let Some(split) = split(row, native, *height) {
            rows.extend(split.rows);
            cells.extend(split.cells);
            heights.extend(split.heights);
            changed = true;
        } else {
            rows.push(row.clone());
            cells.push(native.clone());
            heights.push(*height);
        }
    }
    if !changed {
        return table;
    }
    let model = DocxTableModel { rows };
    let mut text = vec![vec![String::new(); cols]; model.rows.len()];
    let mut underlines = vec![vec![false; cols]; model.rows.len()];
    let mut alignments = vec![vec![ParagraphAlignment::Left; cols]; model.rows.len()];
    for (index, row) in model.rows.iter().enumerate() {
        let mut col = 0;
        for cell in row {
            if col >= cols || col + cell.grid_span.max(1) > cols {
                return table;
            }
            text[index][col] = cell.text.clone();
            underlines[index][col] = cell.underlined;
            alignments[index][col] = cell.alignment;
            col += cell.grid_span.max(1);
        }
        if col != cols {
            return table;
        }
    }
    table.table = OcrTable::with_spans(
        text,
        ocr_spans_from_docx_table_model(&model, model.rows.len(), cols),
    );
    table.cell_underlines = underlines;
    table.cell_alignments = alignments;
    table.docx_table = Some(model);
    table.native_cells = cells;
    table.row_heights_twips = heights;
    // 表示行へ分割した後は、元の物理行gridを同じ行indexの境界として使わない。
    table.source_rule_grid = None;
    table
}

/// 三行以上の同期した本文と、任意の小さい単位行が原本の帯に収まる場合だけ分割する。
fn split(row: &[DocxTableCellModel], native: &[Option<Cell>], height: f32) -> Option<Split> {
    if row.len() != native.len()
        || row.iter().any(|cell| cell.vertical_merge.is_some())
        || !height.is_finite()
        || height <= 0.0
    {
        return None;
    }
    let occupied = row
        .iter()
        .zip(native)
        .filter(|(cell, _)| !cell.text.trim().is_empty())
        .map(|(_, cell)| cell.as_ref())
        .collect::<Option<Vec<_>>>()?;
    if occupied.len() < 2 {
        return None;
    }
    let first = occupied[0];
    let count = occupied.iter().map(|cell| cell.lines.len()).max()?;
    let minimum = occupied.iter().map(|cell| cell.lines.len()).min()?;
    if minimum < 3
        || count - minimum > 1
        || occupied.iter().any(|cell| {
            cell.top != first.top
                || cell.bottom != first.bottom
                || (count != minimum
                    && cell.lines.len() == count
                    && structured_line_max_font_size(&cell.lines[0])
                        >= structured_line_max_font_size(&cell.lines[1]) * 0.8)
        })
    {
        return None;
    }
    let mut edges = vec![first.top];
    for index in 0..count {
        let top = occupied
            .iter()
            .filter_map(|cell| line_at(cell, index, count).map(|line| line.top_y))
            .fold(f32::NEG_INFINITY, f32::max);
        if !top.is_finite()
            || occupied
                .iter()
                .filter_map(|cell| line_at(cell, index, count))
                .any(|line| !line.top_y.is_finite() || top - line.top_y > TEXT_ROW_Y_TOLERANCE_PT)
        {
            return None;
        }
        if index > 0 {
            edges.push(top);
        }
    }
    edges.push(first.bottom);
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut heights = Vec::new();
    let mut used = 0.0;
    let anchors = native
        .iter()
        .map(|cell| {
            cell.as_ref().map(|cell| {
                native_numeric_alignment::right_anchors(&cell.lines, cell.left, cell.right)
            })
        })
        .collect::<Vec<_>>();
    for index in 0..count {
        let row_height = if index + 1 == count {
            height - used
        } else {
            let cumulative = ((first.top - edges[index + 1]) * 20.0).round();
            cumulative - used
        };
        if !row_height.is_finite() || row_height <= 0.0 {
            return None;
        }
        used += row_height;
        let mut next_row = Vec::new();
        let mut next_cells = Vec::new();
        for ((model, original), anchors) in row.iter().zip(native).zip(&anchors) {
            let mut model = model.clone();
            if let Some(borders) = model.borders.as_mut() {
                borders.top &= index == 0;
                borders.bottom &= index + 1 == count;
            }
            let line = original
                .as_ref()
                .and_then(|cell| line_at(cell, index, count));
            let cell = if let Some(line) = line {
                let original = original.as_ref()?;
                let mut cell = original.clone();
                cell.top = edges[index];
                cell.bottom = edges[index + 1];
                cell.lines = vec![line.clone()];
                cell.single_line_runs = line.runs.clone();
                cell.fallback_runs.clear();
                let source_index = index.checked_sub(count.checked_sub(original.lines.len())?)?;
                cell.logical_row_right_anchor = Some(*anchors.as_ref()?.get(source_index)?);
                // 本番と同じ容量・丸め規則を使い、検査で段落IDを生成しない。
                if !line_spacings(&cell)?.all(|spacing| spacing.is_some()) {
                    return None;
                }
                model.text = line.text.clone();
                Some(cell)
            } else {
                model.text.clear();
                model.underlined = false;
                None
            };
            next_row.push(model);
            next_cells.push(cell);
        }
        rows.push(next_row);
        cells.push(next_cells);
        heights.push(row_height);
    }
    Some(Split {
        rows,
        cells,
        heights,
    })
}

/// 単位行を持たない列を先頭の空セルで揃え、本文の各行を一度だけ参照する。
fn line_at(cell: &Cell, index: usize, count: usize) -> Option<&StructuredTextLine> {
    cell.lines
        .get(index.checked_sub(count.checked_sub(cell.lines.len())?)?)
}
