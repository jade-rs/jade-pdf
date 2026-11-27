//! 原本のvector罫線中心の間隔へ、raster bbox由来の余白を足さない回帰。
use super::*;

/// 子表の罫線と終端段落の容量を未確認の親表は、元の行高へ戻す。
#[test]
fn nested_parent_keeps_legacy_height_but_verified_leaf_keeps_source_pitch() {
    let mut parsed = source_grid_selection_tests::source();
    parsed.graphics.clear();
    for (x0, x1, y0, y1) in [(0., 400., 0., 80.), (10., 390., 10., 70.)] {
        for (x0, x1, y0, y1) in [
            (x0, x0, y0, y1),
            (x1, x1, y0, y1),
            (x0, x1, y0, y0),
            (x0, x1, y1, y1),
        ] {
            parsed.graphics.push(ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds { x0, x1, y0, y1 },
                stroke_width: 0.5,
            });
        }
    }
    parsed.graphics.push(ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 200.,
            x1: 200.,
            y0: 10.,
            y1: 70.,
        },
        stroke_width: 0.5,
    });
    parsed.pages[0].width = 440.;
    let template = parsed.text_elements[0].clone();
    parsed.text_elements = [(20., 55.), (210., 55.), (20., 40.), (210., 40.)]
        .into_iter()
        .map(|(x, baseline)| {
            let mut text = template.clone();
            text.baseline_y = Some(baseline);
            text.bounds = ParsedBounds {
                x0: x,
                x1: x + 30.,
                y0: baseline,
                y1: baseline + 10.,
            };
            text
        })
        .collect();
    let tables = infer_positioned_tables_from_pdf_graphics(&parsed);
    assert_eq!(tables.len(), 1);
    let parent = &tables[0];
    assert_eq!(parent.nested_tables.len(), 1);
    assert_eq!(
        parent.bounds,
        ParsedBounds {
            x0: 0.,
            x1: 400.,
            y0: 0.,
            y1: 80.
        }
    );
    let leaf = &parent.nested_tables[0];
    assert!(source_grid_selection::verified(leaf));
    assert_eq!(leaf.row_heights_twips, vec![1200.]);
    assert_eq!(
        parent.row_heights_twips,
        vec![1620.],
        "parent replacement includes an unmeasured nested table"
    );
}

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

/// fontを確認できない表は、従来の薄罫線補正とfallbackを維持する。
#[test]
fn unverified_font_keeps_legacy_height_allowance() {
    let mut parsed = source_grid_selection_tests::source();
    parsed.embedded_fonts.clear();
    let table = infer_positioned_table_from_graphics(&parsed, 0, &parsed.graphics).unwrap();
    assert!(!source_grid_selection::verified(&table));
    assert_eq!(table.row_heights_twips, vec![810., 810.]);
}

/// セル対応を失った途中候補を、元の高さより狭くしない。
#[test]
fn missing_cell_proof_keeps_legacy_height_allowance() {
    let parsed = source_grid_selection_tests::source();
    let mut table = infer_positioned_table_from_graphics(&parsed, 0, &parsed.graphics).unwrap();
    table.native_cells[0][0] = None;
    table.row_heights_twips = vec![810., 810.];
    verified_source_grid_heights::apply(&mut table, &parsed.graphics, &[0, 80, 160], &[0, 40, 80]);
    assert_eq!(table.row_heights_twips, vec![810., 810.]);
}

/// 同じ整数キーへ丸まる二つの原本境界から、架空の正確な行間を選ばない。
#[test]
fn ambiguous_physical_rule_centers_keep_legacy_height_allowance() {
    let mut parsed = source_grid_selection_tests::source();
    parsed.graphics.push(ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 0.,
            x1: 160.,
            y0: 40.1,
            y1: 40.1,
        },
        stroke_width: 0.5,
    });
    let table = infer_positioned_table_from_graphics(&parsed, 0, &parsed.graphics).unwrap();
    assert!(source_grid_selection::verified(&table));
    assert_eq!(table.row_heights_twips, vec![810., 810.]);
}

/// 結合セルの容量を一行高へ誤って読み替えず、既存の配置へ任せる。
#[test]
fn merged_cell_models_keep_legacy_height_allowance() {
    let parsed = source_grid_selection_tests::source();
    let mut table = infer_positioned_table_from_graphics(&parsed, 0, &parsed.graphics).unwrap();
    let mut model = docx_table_model_from_ocr_table(&table.table);
    model.rows[0][0].vertical_merge = Some(DocxVerticalMerge::Restart);
    table.docx_table = Some(model);
    table.row_heights_twips = vec![810., 810.];
    verified_source_grid_heights::apply(&mut table, &parsed.graphics, &[0, 80, 160], &[0, 40, 80]);
    assert_eq!(table.row_heights_twips, vec![810., 810.]);
}

/// 行の容量を照合してから全体を置換し、入り切らない段落を小さい高さへ押し込まない。
#[test]
fn insufficient_new_capacity_keeps_all_previous_heights() {
    let parsed = source_grid_selection_tests::source();
    let mut table = infer_positioned_table_from_graphics(&parsed, 0, &parsed.graphics).unwrap();
    table.source_rule_grid = Some(fractional_table_rules::Grid {
        xs: vec![0., 80., 160.],
        ys: vec![0., 20., 40.],
    });
    table.row_heights_twips = vec![810., 810.];
    verified_source_grid_heights::apply(&mut table, &parsed.graphics, &[0, 80, 160], &[0, 40, 80]);
    assert_eq!(table.row_heights_twips, vec![810., 810.]);
}

/// 行boxが罫線中心まで使い切るときは、Word罫線の内側へ押し込まない。
#[test]
fn full_line_box_at_border_keeps_legacy_height_allowance() {
    let mut parsed = source_grid_selection_tests::source();
    for text in &mut parsed.text_elements {
        if matches!(text.baseline_y, Some(45. | 5.)) {
            let baseline = text.baseline_y.unwrap() - 5.;
            text.baseline_y = Some(baseline);
            text.bounds.y0 = baseline;
            text.bounds.y1 = baseline + text.font_size;
        }
    }
    let table = infer_positioned_table_from_graphics(&parsed, 0, &parsed.graphics).unwrap();
    assert!(source_grid_selection::verified(&table));
    assert_eq!(
        table.row_heights_twips,
        vec![810., 810.],
        "full source line boxes need the existing border allowance"
    );
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 整数・小数の平行移動と行間の拡大でも、原本の中心間隔だけを保持する。
    #[test]
    fn verified_source_pitch_is_independent_of_translation_and_stroke_width(
        scale in 1u16..8, dy_quarters in 0u16..128, stroke_eighths in 1u16..6
    ) {
        let mut parsed=source_grid_selection_tests::source();
        let scale=f32::from(scale);let dy=f32::from(dy_quarters)/4.;
        for graphic in &mut parsed.graphics {
            graphic.bounds.y0=graphic.bounds.y0*scale+dy;
            graphic.bounds.y1=graphic.bounds.y1*scale+dy;
            graphic.stroke_width=f32::from(stroke_eighths)/8.;
        }
        for text in &mut parsed.text_elements {
            let baseline=text.baseline_y.unwrap()*scale+dy;
            text.baseline_y=Some(baseline);text.bounds.y0=baseline;text.bounds.y1=baseline+text.font_size;
        }
        parsed.pages[0].height=120.*scale+dy;
        let table=infer_positioned_table_from_graphics(&parsed,0,&parsed.graphics).unwrap();
        prop_assert!(source_grid_selection::verified(&table));
        prop_assert_eq!(table.row_heights_twips,vec![800.*scale,800.*scale]);
    }
}
