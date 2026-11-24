//! 表の先頭行を後続行から切り離さない、一般的なWord改ページ指定の回帰。
use super::*;
use docx_rs::BuildXML;

/// 編集可能な二列の所有表から、Wordへ渡すXMLを取り出す。
fn table_xml(rows: usize, merged: bool) -> String {
    let mut table = OcrTable::new(
        (0..rows)
            .map(|r| vec![format!("left{r}"), format!("right{r}")])
            .collect(),
    );
    if merged && rows > 0 {
        table.spans[0][0] = OcrCellSpan::new(1, 2);
        table.rows[0][1].clear();
    }
    String::from_utf8(docx_table_from_ocr_table(&table).build()).unwrap()
}

/// 通常のデータ行を繰り返し見出しにせず、先頭行の段落だけ次へつなぐ。
#[test]
fn opening_table_row_stays_with_body_without_becoming_repeated_header() {
    let xml = table_xml(3, false);
    let rows = xml.split("<w:tr>").skip(1).collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].matches("<w:keepNext").count(),
        2,
        "opening table cells must stay with the next row: {xml}"
    );
    assert!(!rows[1].contains("<w:keepNext"));
    assert!(!rows[2].contains("<w:keepNext"));
    assert!(!xml.contains("<w:tblHeader"));
    assert!(!xml.contains("<w:cantSplit"));
}

/// 横結合した先頭セルも、結合の構造を変えずに次の行へつなぐ。
#[test]
fn merged_opening_cell_keeps_its_span_and_next_row() {
    let xml = table_xml(2, true);
    let first = xml.split("<w:tr>").nth(1).unwrap();
    assert!(first.contains("<w:gridSpan w:val=\"2\""));
    assert_eq!(
        first.matches("<w:keepNext").count(),
        1,
        "merged opening cell must stay with the next row: {xml}"
    );
}

/// 一行だけの表に後続行を仮定して、後ろの本文を拘束しない。
#[test]
fn single_row_table_does_not_keep_unrelated_following_body() {
    assert!(!table_xml(1, false).contains("<w:keepNext"));
    assert!(!table_xml(1, true).contains("<w:keepNext"));
}
