//! 離れた黒枠と枠なしの値を、元の行・列へ一意に対応させる回帰。
use super::*;

/// 四つのデータ欄のうち三欄だけに独立した枠を持つ、所有された表IRを作る。
fn boxed_source(
    count: usize,
    horizontal_gap: f32,
    vertical_gap: f32,
) -> (ParsedDocument, Vec<Vec<String>>, [f32; 4], Vec<f32>) {
    let mut parsed = source(2, &[]);
    parsed.page_count = 1;
    parsed.pages.truncate(1);
    parsed.pages[0].width = 842.;
    parsed.pages[0].height = 1191.;
    parsed.text_elements.clear();
    let x0 = 20.;
    let x1 = x0 + 80. + horizontal_gap;
    let x2 = x1 + 80.;
    let x3 = x2 + 60. + 2. * horizontal_gap;
    let centers = [x0 + 40., x1 + 40., x2 + 30. + horizontal_gap, x3 + 140.];
    let mut rows = Vec::new();
    let mut ys = Vec::new();
    for row in 0..count {
        let y = 900. - row as f32 * (24. + vertical_gap);
        ys.push(y + 12.);
        let values = vec![
            format!("10{}", row + 3),
            format!("name{row}"),
            if row == 1 {
                String::new()
            } else {
                format!("S{row}")
            },
            format!("1{}", "0".repeat(row + 3)),
        ];
        for (left, width) in [(x0, 80.), (x1, 80.), (x3, 280.)] {
            for (a, b, c, d) in [
                (left, y, left + width, y),
                (left, y + 24., left + width, y + 24.),
                (left, y, left, y + 24.),
                (left + width, y, left + width, y + 24.),
            ] {
                parsed.graphics.push(ParsedGraphicElement {
                    page: 0,
                    bounds: ParsedBounds {
                        x0: a,
                        y0: b,
                        x1: c,
                        y1: d,
                    },
                    stroke_width: 0.5,
                    dashed: false,
                });
            }
        }
        for col in 0..4 {
            if values[col].is_empty() {
                continue;
            }
            let (text, left, width, size, baseline) = if col == 0 {
                ("10".to_owned(), x0 + 23., 13.4, 12., y + 6.)
            } else {
                let width = values[col].len() as f32 * 6.7;
                (
                    values[col].clone(),
                    centers[col] - width / 2.,
                    width,
                    12.,
                    y + 6.,
                )
            };
            parsed.text_elements.push(ParsedTextElement {
                page: 0,
                text,
                font_name: Some("Helvetica".into()),
                font_size: size,
                fill_color: Some("000000".into()),
                baseline_y: Some(baseline),
                bounds: ParsedBounds {
                    x0: left,
                    y0: baseline,
                    x1: left + width,
                    y1: baseline + size,
                },
            });
        }
        parsed.text_elements.push(ParsedTextElement {
            page: 0,
            text: (row + 3).to_string(),
            font_name: Some("Helvetica".into()),
            font_size: 7.,
            fill_color: Some("000000".into()),
            baseline_y: Some(y + 11.),
            bounds: ParsedBounds {
                x0: x0 + 36.4,
                y0: y + 11.,
                x1: x0 + 40.4,
                y1: y + 18.,
            },
        });
        rows.push(values);
    }
    (parsed, rows, centers, ys)
}

/// 明示された物理gridを使って、元のデータ欄の中心が入るセル番号を求める。
fn physical_column(table: &PositionedTable, x: f32) -> usize {
    assert!(
        !table.column_widths_twips.is_empty(),
        "source box widths must remain explicit"
    );
    let mut left = table.bounds.x0;
    table
        .column_widths_twips
        .iter()
        .position(|width| {
            let right = left + *width as f32 / 20.;
            let inside = left <= x && x < right;
            left = right;
            inside
        })
        .expect("source column center must belong to a unique output cell")
}

/// 明示された物理行高を使い、空白のguttersをデータ行と混同しない。
fn physical_row(table: &PositionedTable, y: f32) -> usize {
    assert!(
        !table.row_heights_twips.is_empty(),
        "source box rows must remain explicit"
    );
    let mut top = table.bounds.y1;
    table
        .row_heights_twips
        .iter()
        .position(|height| {
            let bottom = top - *height / 20.;
            let inside = bottom < y && y <= top;
            top = bottom;
            inside
        })
        .expect("source row center must belong to a unique output row")
}

/// 空のデータ欄は削らず、明示的な物理gutterだけを別セルとして許す。
fn assert_source_cells(
    parsed: &ParsedDocument,
    expected: &[Vec<String>],
    xs: [f32; 4],
    ys: &[f32],
) {
    let tables = infer_positioned_tables_from_pdf_graphics(parsed);
    assert_eq!(
        tables.len(),
        1,
        "separated source boxes must form one row-associated table"
    );
    let table = &tables[0];
    let cols = xs.map(|x| physical_column(table, x));
    assert_eq!(
        cols.into_iter().collect::<BTreeSet<_>>().len(),
        4,
        "all four source columns, including the borderless one, must remain distinct"
    );
    for (row, values) in expected.iter().enumerate() {
        let actual = physical_row(table, ys[row]);
        for col in 0..4 {
            assert_eq!(
                table.table.rows[actual][cols[col]]
                    .split_whitespace()
                    .collect::<String>(),
                values[col],
                "source cell row={row} col={col} must not move or disappear"
            );
        }
    }
}

/// 右の枠だけを一セル表として先行させず、指数・名前・空欄・数値を同じ行へ置く。
#[test]
fn separated_box_columns_keep_their_source_rows_and_empty_cell() {
    let (parsed, expected, xs, ys) = boxed_source(3, 20., 16.);
    assert_source_cells(&parsed, &expected, xs, &ys);
    let tables = infer_positioned_tables_from_pdf_graphics(&parsed);
    let mut parsed = parsed;
    let summary = tables.first().map(|table| table.table.clone());
    let docx = build_structured_docx_from_owned_images(&mut parsed, &summary, &tables).unwrap();
    let bytes = pack_docx_to_bytes(docx, &[]).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(
        xml.matches("<w:tbl>").count(),
        1,
        "the editable source table must not become separate boxes or nested exponent tables"
    );
    assert!(
        !xml.contains("<w:u "),
        "closed source box borders must not become text underlines"
    );
    assert_eq!(
        xml.matches("<w:position w:val=\"10\"").count(),
        3,
        "each factor exponent must retain its source5pt rise inside its own cell"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 行数と縦横の白い間隔を変えても、glyphの欄と空欄の所属を保持する。
    #[test]
    fn spaced_box_cells_keep_ownership(rows in 3_usize..6,horizontal_gap in 10_u8..25,vertical_gap in 10_u8..23) {
        let (parsed,expected,xs,ys)=boxed_source(rows,f32::from(horizontal_gap),f32::from(vertical_gap));
        assert_source_cells(&parsed,&expected,xs,&ys);
    }
}

/// 閉じた枠のある列でも、一つの空欄を消したり隣の列へ詰めたりしない。
#[test]
fn an_empty_outlined_cell_preserves_its_row_and_column() {
    let (mut parsed, mut expected, xs, ys) = boxed_source(3, 20., 16.);
    parsed.text_elements.retain(|text| text.text != "name1");
    expected[1][1].clear();
    assert_source_cells(&parsed, &expected, xs, &ys);
}

/// 現行point gridで表せない端を、罫線のない別のセルとして黙って出力しない。
#[test]
fn fractional_edges_do_not_silently_drop_the_source_borders() {
    let (mut parsed, _, _, _) = boxed_source(3, 20., 16.);
    for graphic in &mut parsed.graphics {
        graphic.bounds.x0 += 0.25;
        graphic.bounds.x1 += 0.25;
    }
    for text in &mut parsed.text_elements {
        text.bounds.x0 += 0.25;
        text.bounds.x1 += 0.25;
    }
    assert!(
        separated_box_table::infer(&parsed, 0, &parsed.graphics).is_none(),
        "unrepresentable source edges must retain the existing fallback instead of losing borders"
    );
}

/// 境界の両側へ跨る文字を、近接する片側セルの本文として所有させない。
#[test]
fn a_fragment_crossing_two_physical_cells_rejects_the_new_model() {
    let (mut parsed, _, _, _) = boxed_source(3, 20., 16.);
    let text = parsed
        .text_elements
        .iter_mut()
        .find(|t| t.text == "name0")
        .unwrap();
    text.bounds.x0 = 199.75;
    text.bounds.x1 = 200.25;
    assert!(
        separated_box_table::infer(&parsed, 0, &parsed.graphics).is_none(),
        "ambiguous source glyphs must not be assigned across a cell edge"
    );
}

/// 欠けた辺・余分な線・縦の白帯の本文・反復しない枠は、閉じた表の証拠に使わない。
#[test]
fn incomplete_or_nonrepeating_boxes_retain_the_existing_fallback() {
    for case in 0..5 {
        let (mut parsed, _, _, _) = boxed_source(3, 20., 16.);
        match case {
            0 => {
                parsed.graphics.remove(0);
            }
            1 => {
                let mut extra = parsed.graphics[0].clone();
                extra.bounds.y0 = 100.;
                extra.bounds.y1 = 100.;
                parsed.graphics.push(extra);
            }
            2 => {
                let mut text = parsed.text_elements[0].clone();
                text.bounds.y0 = 888.;
                text.bounds.y1 = 896.;
                text.baseline_y = Some(888.);
                parsed.text_elements.push(text);
            }
            3 => {
                for g in &mut parsed.graphics[..4] {
                    g.bounds.x0 += 2.;
                    g.bounds.x1 += 2.;
                }
            }
            _ => {
                parsed.text_elements.clear();
            }
        }
        assert!(
            separated_box_table::infer(&parsed, 0, &parsed.graphics).is_none(),
            "uncertain source grid case {case} must not manufacture a table"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 同じ文字列が別の欄に反復しても、paint順に依存せず物理位置で所有を決める。
    #[test]
    fn repeated_values_and_paint_order_keep_source_cell_identity(rotation in 0_usize..97, reverse in any::<bool>()) {
        let (mut parsed,mut expected,xs,ys)=boxed_source(4,20.,16.);
        for row in &mut expected { row[1]="same".into(); }
        for text in &mut parsed.text_elements {if text.text.starts_with("name"){text.text="same".into();}}
        let len=parsed.graphics.len();parsed.graphics.rotate_left(rotation%len);
        let len=parsed.text_elements.len();parsed.text_elements.rotate_left(rotation%len);
        if reverse {parsed.graphics.reverse();parsed.text_elements.reverse();}
        assert_source_cells(&parsed,&expected,xs,&ys);
        let tables=infer_positioned_tables_from_pdf_graphics(&parsed);
        let table=&tables[0];
        let model=table.docx_table.as_ref().unwrap();
        prop_assert_eq!(model.rows.len(),7);
        prop_assert_eq!(&table.column_widths_twips,&vec![1600,400,1600,2000,5600]);
        for (row, cells) in model.rows.iter().enumerate() {
            for (col,cell) in cells.iter().enumerate() {
                let b=cell.borders.unwrap();
                // 実データ行には三つの黒い枠。縦の白帯には縦罫線を足さない。
                if row%2==0 {
                    prop_assert_eq!((b.top,b.bottom),if col%2==0{(true,true)}else{(false,false)});
                    prop_assert!(b.left && b.right);
                } else {
                    prop_assert!(!b.left && !b.right);
                    prop_assert_eq!((b.top,b.bottom),if col%2==0{(true,true)}else{(false,false)});
                }
                prop_assert_eq!(b.size_eighths_pt,4);
            }
        }
    }
}
