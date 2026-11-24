//! 別ページの表が実段組を無効化せず、表末尾にも原本ページ境界を保持する回帰。
use super::*;

/// 自作の2行2列の表を、原本ページ末尾に配置する。
fn positioned_table(page: usize) -> PositionedTable {
    PositionedTable {
        page,
        bounds: ParsedBounds {
            x0: 48.,
            y0: 500.,
            x1: 248.,
            y1: 560.,
        },
        stroke_width: 0.5,
        font_name: Some("Helvetica".into()),
        font_embedded: false,
        embedded_face: None,
        font_size: Some(11.),
        column_widths_twips: vec![2000; 2],
        row_heights_twips: vec![600.; 2],
        table: OcrTable::new(
            (0..2)
                .map(|row| {
                    (0..2)
                        .map(|column| format!("TABLE PAGE{page} R{row}C{column}"))
                        .collect()
                })
                .collect(),
        ),
        cell_underlines: vec![vec![false; 2]; 2],
        cell_alignments: vec![vec![ParagraphAlignment::Left; 2]; 2],
        docx_table: None,
        native_cells: vec![vec![None; 2]; 2],
        nested_tables: Vec::new(),
    }
}

/// 一方だけに表を持つ原本を所有データから梱包し、実際の本文XMLを読む。
fn table_and_columns(table_page: usize) -> String {
    let mut parsed = source(2, &[]);
    parsed
        .text_elements
        .retain(|text| text.page != table_page || text.bounds.x0 == 48.);
    let docx = build_structured_docx_from_owned_images(
        &mut parsed,
        &None,
        &[positioned_table(table_page)],
    )
    .unwrap();
    let bytes = pack_docx_to_bytes(docx, &[]).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 最終ページの表のために前ページの左右段を一つの縦列へ流さない。
#[test]
fn table_on_last_page_preserves_earlier_columns_and_cells() {
    let xml = table_and_columns(1);
    assert_eq!(
        xml.matches("w:num=\"2\"").count(),
        1,
        "table on another page must not erase columns"
    );
    assert_eq!(xml.matches("<w:sectPr").count(), 2);
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    for row in 0..2 {
        for column in 0..2 {
            assert_eq!(
                xml.matches(&format!("TABLE PAGE1 R{row}C{column}")).count(),
                1
            );
        }
    }
    assert!(xml.contains("PAGEA LEFT 1") && xml.contains("PAGEA RIGHT 1"));
}
