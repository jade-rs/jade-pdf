//! 太さを変えた水平罫線の中心を、累積行境界へ戻して表全高を保つ。
use super::*;

/// 原本段落容量と外枠位置を保てる場合だけ行高を補正し、未確認の水平精密化は戻す。
pub(super) fn apply(table: &mut PositionedTable, model: &mut DocxTableModel) {
    let horizontal = model.rows.iter().flatten().any(|cell| {
        cell.borders
            .is_some_and(|b| b.source_side_sizes[0].is_some() || b.source_side_sizes[2].is_some())
    });
    if !horizontal {
        return;
    }
    let heights = heights(table, model).filter(|heights| {
        table.native_cells.len() == heights.len()
            && table.native_cells.iter().zip(heights).all(|(row, height)| {
                row.iter()
                    .flatten()
                    .all(|cell| native_table_text::fits_height_twips(cell, *height))
            })
    });
    if let Some(heights) = heights {
        table.row_heights_twips = heights;
    } else {
        for cell in model.rows.iter_mut().flatten() {
            if let Some(borders) = &mut cell.borders {
                borders.source_side_sizes[0] = None;
                borders.source_side_sizes[2] = None;
            }
        }
    }
}

/// 同じ行境界の上下の辺を照合し、全列で同じ実効Word幅を持つ場合だけ返す。
fn boundary(model: &DocxTableModel, row: usize, default: usize) -> Option<usize> {
    let cols = model.rows.first()?.len();
    let mut width = None;
    for col in 0..cols {
        let above = if row > 0 {
            model
                .rows
                .get(row - 1)?
                .get(col)?
                .borders?
                .source_side_sizes[2]
                .unwrap_or(default)
        } else {
            default
        };
        let below = if row < model.rows.len() {
            model.rows.get(row)?.get(col)?.borders?.source_side_sizes[0].unwrap_or(default)
        } else {
            default
        };
        let current = if row == 0 {
            below
        } else if row == model.rows.len() {
            above
        } else {
            above.max(below)
        };
        if width.is_some_and(|old| old != current) {
            return None;
        }
        width = Some(current);
    }
    width
}

/// 境界ごとの半線幅差をtwipへ累積丸めし、隣接差だけを各行高へ加える。
fn heights(table: &PositionedTable, model: &DocxTableModel) -> Option<Vec<f32>> {
    let rows = model.rows.len();
    if rows == 0 || table.row_heights_twips.len() != rows {
        return None;
    }
    let default = table_border_size_for_pdf_rule(table.stroke_width);
    if boundary(model, 0, default)? != default || boundary(model, rows, default)? != default {
        return None;
    }
    let mut previous = 0.;
    let mut heights = Vec::with_capacity(rows);
    for (row, height) in table.row_heights_twips.iter().enumerate() {
        let width = boundary(model, row + 1, default)?;
        if width > default {
            return None;
        }
        // (代表幅 - 実効幅) /8pt /2 *20twip = 幅差 *1.25twip。
        let current = ((default - width) as f64 * 1.25).round();
        let actual = f64::from(*height) + current - previous;
        if !actual.is_finite() || actual < 1. || actual > f64::from(i32::MAX) {
            return None;
        }
        heights.push(actual as f32);
        previous = current;
    }
    (previous == 0.).then_some(heights)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表外側には隣接セルがないため、架空の太い隣辺で実際の外枠幅を隠さない。
    #[test]
    fn outer_boundaries_do_not_borrow_an_absent_neighbor() {
        let mut model = docx_table_model_from_ocr_table(&OcrTable::new(vec![vec!["Owned".into()]]));
        let borders = model.rows[0][0].borders.as_mut().unwrap();
        borders.source_side_sizes[0] = Some(4);
        borders.source_side_sizes[2] = Some(4);
        assert_eq!(boundary(&model, 0, 12), Some(4));
        assert_eq!(boundary(&model, 1, 12), Some(4));
    }
}
