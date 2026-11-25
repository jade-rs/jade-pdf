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
    table
}

/// どの非空列にも同じ三行以上があり、各行boxが原本の帯に収まる場合だけ分割する。
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
    let count = first.lines.len();
    if count < 3
        || occupied.iter().any(|cell| {
            cell.lines.len() != count || cell.top != first.top || cell.bottom != first.bottom
        })
    {
        return None;
    }
    let mut edges = vec![first.top];
    for index in 0..count {
        let top = occupied
            .iter()
            .map(|cell| cell.lines[index].top_y)
            .fold(f32::NEG_INFINITY, f32::max);
        if !top.is_finite()
            || occupied.iter().any(|cell| {
                let line = &cell.lines[index];
                !line.top_y.is_finite() || top - line.top_y > TEXT_ROW_Y_TOLERANCE_PT
            })
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
        for (model, original) in row.iter().zip(native) {
            let mut model = model.clone();
            if let Some(borders) = model.borders.as_mut() {
                borders.top &= index == 0;
                borders.bottom &= index + 1 == count;
            }
            let cell = if model.text.trim().is_empty() {
                None
            } else {
                let original = original.as_ref()?;
                let line = original.lines.get(index)?;
                let mut cell = original.clone();
                cell.top = edges[index];
                cell.bottom = edges[index + 1];
                cell.lines = vec![line.clone()];
                cell.single_line_runs = line.runs.clone();
                cell.fallback_runs.clear();
                cell.logical_row = true;
                // fit検証も本番と同じbuilderを通し、行boxの欠落を許さない。
                table_cell(&cell)?;
                model.text = line.text.clone();
                Some(cell)
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
