//! 表の先頭行と本文をWordの改ページで離さない。
use docx_rs::{Table, TableCellContent, TableChild, TableRowChild};

/// 二行以上の表の先頭セルでは、末尾の直接段落だけを次の行へつなぐ。
/// 行全体の分割禁止や繰返し見出しは追加せず、入れ子の表へは降りない。
pub(super) fn keep_with_next(mut table: Table) -> Table {
    if table.rows.len() < 2 {
        return table;
    }
    if let Some(TableChild::TableRow(row)) = table.rows.first_mut() {
        for TableRowChild::TableCell(cell) in &mut row.cells {
            if let Some(TableCellContent::Paragraph(paragraph)) = cell.children.last_mut() {
                paragraph.property.keep_next = Some(true);
            }
        }
    }
    table
}
