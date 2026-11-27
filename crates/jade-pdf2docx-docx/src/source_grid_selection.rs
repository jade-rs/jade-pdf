//! 表示行数の利得より、原本セル内に収まる確認済み本文・fontの所有を優先する。
use super::*;

/// 空の表や未確認セルを除き、結合のない完成グリッドにも既存の原本配置検査を適用する。
pub(super) fn verified(table: &PositionedTable) -> bool {
    if ocr_table_non_empty_cell_count(&table.table) == 0 {
        return false;
    }
    let fallback;
    let model = match &table.docx_table {
        Some(model) => model,
        None => {
            fallback = docx_table_model_from_ocr_table(&table.table);
            &fallback
        }
    };
    native_table_text::fractional_geometry_is_safe(&table.native_cells, model)
}
