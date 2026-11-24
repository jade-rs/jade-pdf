//! 離れた黒枠と枠なしの値を、元の行・列へ一意に対応させる回帰。
use super::*;
use proptest::prelude::*;

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
    let (xml, _) = package_source(parsed);
    assert_eq!(
        xml.matches("<w:tbl>").count(),
        1,
        "the editable source table must not become separate boxes or nested exponent tables"
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
