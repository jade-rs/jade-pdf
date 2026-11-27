//! 原本のvector罫線中心の間隔へ、raster bbox由来の余白を足さない回帰。
use super::*;

/// fontとセル内の行容量を確認済みなら、40ptの原本行を40.5ptへ伸ばさない。
#[test]
fn verified_native_grid_rows_do_not_add_raster_bbox_padding() {
    let parsed = source_grid_selection_tests::source();
    let table = infer_positioned_table_from_graphics(&parsed, 0, &parsed.graphics).unwrap();
    assert_eq!(table.table.row_count(), 2);
    assert_eq!(table.table.col_count(), Some(2));
    assert_eq!(table.stroke_width, 0.5);
    assert!(source_grid_selection::verified(&table));
    assert!(table.native_cells.iter().flatten().all(|cell| {
        cell.as_ref()
            .is_some_and(|cell| native_table_text::fits_height_twips(cell, 800.))
    }));
    assert_eq!(
        table.row_heights_twips,
        vec![800., 800.],
        "verified source rule centers already define the physical row pitch"
    );
}
