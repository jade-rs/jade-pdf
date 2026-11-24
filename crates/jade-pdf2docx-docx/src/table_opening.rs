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

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::{Paragraph, Run, TableCell, TableRow};
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 後続行・先行段落・セル属性を保ち、何回適用しても指定は増えない。
        #[test]
        fn only_last_paragraph_of_opening_cells_changes(rows in 2_usize..6, cols in 1_usize..6, paragraphs in 1_usize..5) {
            let original=Table::new((0..rows).map(|r|TableRow::new((0..cols).map(|c|{
                (0..paragraphs).fold(TableCell::new(),|cell,p|cell.add_paragraph(Paragraph::new().add_run(Run::new().add_text(format!("{r}:{c}:{p}")))))
            }).collect())).collect());
            let actual=keep_with_next(original.clone());
            prop_assert_eq!(&actual.rows[1..],&original.rows[1..]);
            prop_assert_eq!(&actual.property,&original.property);
            prop_assert_eq!(&actual.grid,&original.grid);
            let TableChild::TableRow(first)=&actual.rows[0];
            let TableChild::TableRow(before)=&original.rows[0];
            for (TableRowChild::TableCell(cell),TableRowChild::TableCell(old)) in first.cells.iter().zip(&before.cells) {
                prop_assert_eq!(&cell.property,&old.property);
                prop_assert_eq!(&cell.children[..paragraphs-1],&old.children[..paragraphs-1]);
                let TableCellContent::Paragraph(p)=cell.children.last().unwrap() else {unreachable!()};
                prop_assert_eq!(p.property.keep_next,Some(true));
                prop_assert_eq!(&p.children,match old.children.last().unwrap(){TableCellContent::Paragraph(p)=>&p.children,_=>unreachable!()});
            }
            prop_assert_eq!(keep_with_next(actual.clone()),actual);
        }
    }

    /// 末尾が子表のセルでは、その中の段落を親行の指定で変更しない。
    #[test]
    fn terminal_nested_table_does_not_receive_parent_row_keep() {
        let child = Table::new(vec![TableRow::new(vec![
            TableCell::new().add_paragraph(Paragraph::new()),
        ])]);
        let original = Table::new(vec![
            TableRow::new(vec![TableCell::new().add_table(child)]),
            TableRow::new(vec![TableCell::new().add_paragraph(Paragraph::new())]),
        ]);
        assert_eq!(keep_with_next(original.clone()), original);
    }
}
