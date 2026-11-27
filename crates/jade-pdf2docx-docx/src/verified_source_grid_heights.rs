//! 確認済みvector gridでは、raster bboxの薄罫線余白を原本行間へ重ねない。
use super::*;

/// 原本軸・全非空セル・font・新しい行容量がそろう非結合表だけを精密化する。
pub(super) fn apply(
    table: &mut PositionedTable,
    graphics: &[ParsedGraphicElement],
    xs: &[i32],
    ys: &[i32],
) {
    if !source_grid_selection::verified(table)
        || table.docx_table.as_ref().is_some_and(|model| {
            model.rows.iter().flatten().any(|cell| {
                cell.grid_span != 1
                    || cell.vertical_merge.is_some()
                    || cell
                        .borders
                        .is_none_or(|b| !(b.top && b.left && b.bottom && b.right))
            })
        })
    {
        return;
    }
    let fallback;
    let grid = if let Some(grid) = &table.source_rule_grid {
        grid
    } else {
        let Some(grid) = fractional_table_rules::physical_grid(graphics, xs, ys, Some(table.page))
        else {
            return;
        };
        fallback = grid;
        &fallback
    };
    let Some(heights) = grid.heights(0.) else {
        return;
    };
    // 上下のWord罫線の半幅ずつを予約し、center間隔の全高を文字の内側容量にしない。
    let border_twips = table_border_size_for_pdf_rule(table.stroke_width) as f32 * 2.5;
    if heights.len() == table.native_cells.len()
        && heights
            .iter()
            .zip(&table.native_cells)
            .all(|(height, row)| {
                row.iter()
                    .flatten()
                    .all(|cell| native_table_text::fits_height_twips(cell, *height - border_twips))
            })
    {
        table.row_heights_twips = heights;
    }
}
