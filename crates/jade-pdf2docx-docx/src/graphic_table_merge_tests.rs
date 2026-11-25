//! 部分罫線の接合部でも、結合セルが矩形として一意に所有されることを確認する。
use super::*;
use proptest::prelude::*;

/// 指定された水平・垂直辺を持つ小さい所有gridを作る。
fn grid_graphics(
    xs: &[i32],
    ys: &[i32],
    vertical: &[Vec<bool>],
    horizontal: &[Vec<bool>],
) -> Vec<ParsedGraphicElement> {
    let mut out = Vec::new();
    for (row, edges) in vertical.iter().enumerate() {
        let lo = ys.len() - 2 - row;
        for (col, on) in edges.iter().enumerate() {
            if *on {
                out.push(ParsedGraphicElement {
                    page: 0,
                    stroke_width: 0.5,
                    dashed: false,
                    bounds: ParsedBounds {
                        x0: xs[col] as f32,
                        x1: xs[col] as f32,
                        y0: ys[lo] as f32,
                        y1: ys[lo + 1] as f32,
                    },
                });
            }
        }
    }
    for (edge, cells) in horizontal.iter().enumerate() {
        let y = ys[ys.len() - 1 - edge];
        for (col, on) in cells.iter().enumerate() {
            if *on {
                out.push(ParsedGraphicElement {
                    page: 0,
                    stroke_width: 0.5,
                    dashed: false,
                    bounds: ParsedBounds {
                        x0: xs[col] as f32,
                        x1: xs[col + 1] as f32,
                        y0: y as f32,
                        y1: y as f32,
                    },
                });
            }
        }
    }
    out
}

/// Wordの継続セルには、直前の行の同じ列・同じ横幅に開始または継続セルが必要。
fn assert_rectangular_continuations(model: &DocxTableModel, cols: usize) {
    let mut previous = BTreeMap::new();
    for row in &model.rows {
        let mut col = 0;
        let mut current = BTreeMap::new();
        for cell in row {
            let span = cell.grid_span.max(1);
            if cell.vertical_merge == Some(DocxVerticalMerge::Continue) {
                assert!(
                    previous.get(&(col, span)).is_some_and(|v| matches!(
                        v,
                        DocxVerticalMerge::Restart | DocxVerticalMerge::Continue
                    )),
                    "vertical continuation must have an identical rectangular predecessor"
                );
                assert!(
                    cell.text.is_empty(),
                    "continuation must not hide source text below its restart owner"
                );
            }
            if let Some(merge) = cell.vertical_merge {
                current.insert((col, span), merge);
            }
            col += span;
        }
        assert_eq!(
            col, cols,
            "each physical grid band must be covered exactly once"
        );
        previous = current;
    }
}

/// 結合矩形の外周だけを展開し、元の全小区間の罫線を欠落・追加なしで照合する。
fn assert_original_edges(model: &DocxTableModel, vertical: &[Vec<bool>], horizontal: &[Vec<bool>]) {
    let rows = vertical.len();
    let cols = horizontal[0].len();
    let mut v = vec![vec![false; cols + 1]; rows];
    let mut h = vec![vec![false; cols]; rows + 1];
    for (ri, row) in model.rows.iter().enumerate() {
        let mut col = 0;
        for cell in row {
            let span = cell.grid_span;
            if cell.vertical_merge != Some(DocxVerticalMerge::Continue) {
                let height = if cell.vertical_merge == Some(DocxVerticalMerge::Restart) {
                    vertical_merge_row_span(model, ri, col, rows)
                } else {
                    1
                };
                let b = cell.borders.unwrap();
                for c in col..col + span {
                    h[ri][c] |= b.top;
                    h[ri + height][c] |= b.bottom;
                }
                for r in ri..ri + height {
                    v[r][col] |= b.left;
                    v[r][col + span] |= b.right;
                }
            }
            col += span;
        }
    }
    assert_eq!(
        h, horizontal,
        "model must preserve each original atomic border"
    );
    assert_eq!(
        v, vertical,
        "model must preserve each original atomic border"
    );
}

/// T字の接合で上の横結合と下の縦結合を重ねず、欠けた辺を追加しない。
#[test]
fn t_junction_does_not_create_an_orphan_vertical_continuation() {
    let xs = [0, 20, 40];
    let ys = [0, 20, 40];
    let g = grid_graphics(
        &xs,
        &ys,
        &[vec![true, false, true], vec![true, true, true]],
        &[vec![true, true], vec![false, true], vec![true, true]],
    );
    let cells = vec![
        vec!["top".into(), String::new()],
        vec!["lower".into(), "side".into()],
    ];
    let model = docx_table_model_from_grid(
        &g,
        &xs,
        &ys,
        &cells,
        &vec![vec![false; 2]; 2],
        &vec![vec![None; 2]; 2],
    )
    .unwrap();
    assert_rectangular_continuations(&model, 2);
}

/// 縦に結合した全行の文字は開始セルへ移し、継続セルに隠さない。
#[test]
fn vertical_cell_text_belongs_to_the_restart_owner() {
    let xs = [0, 20, 40];
    let ys = [0, 20, 40];
    let g = grid_graphics(
        &xs,
        &ys,
        &[vec![true, true, true], vec![true, true, true]],
        &[vec![true, true], vec![false, true], vec![true, true]],
    );
    let cells = vec![
        vec!["top".into(), "rightA".into()],
        vec!["lower".into(), "rightB".into()],
    ];
    let model = docx_table_model_from_grid(
        &g,
        &xs,
        &ys,
        &cells,
        &vec![vec![false; 2]; 2],
        &vec![vec![None; 2]; 2],
    )
    .unwrap();
    assert_rectangular_continuations(&model, 2);
    let root = &model.rows[0][0];
    assert_eq!(root.vertical_merge, Some(DocxVerticalMerge::Restart));
    assert_eq!(
        root.text.split_whitespace().collect::<Vec<_>>(),
        vec!["top", "lower"],
        "merged source text must be visible in its restart owner"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意の内部罫線でも、継続は一意の矩形だけを指し、文字列を欠落・複製しない。
    #[test]
    fn arbitrary_partial_rules_have_rectangular_text_owners(bits in any::<u32>()) {
        let xs=[0,20,40,60];let ys=[0,20,40,60];
        let mut vertical=vec![vec![true;4];3];let mut horizontal=vec![vec![true;3];4];
        for row in 0..3 {for col in 1..3 {vertical[row][col]=bits&(1<<(row*2+col-1))!=0;}}
        for row in 1..3 {for col in 0..3 {horizontal[row][col]=bits&(1<<(6+(row-1)*3+col))!=0;}}
        let g=grid_graphics(&xs,&ys,&vertical,&horizontal);
        let cells=(0..3).map(|r|(0..3).map(|c|if bits&(1<<(12+r*3+c))==0 {String::new()}else{format!("R{r}C{c}")}).collect::<Vec<_>>()).collect::<Vec<_>>();
        if let Some(model)=docx_table_model_from_grid(&g,&xs,&ys,&cells,&vec![vec![false;3];3],&vec![vec![None;3];3]) {
            assert_rectangular_continuations(&model,3);
            assert_original_edges(&model,&vertical,&horizontal);
            prop_assert!(model_preserves_grid_cell_texts(&model,&cells));
            let expected=cells.iter().flatten().filter(|s|!s.is_empty()).cloned().collect::<BTreeSet<_>>();
            let actual=model.rows.iter().flatten().flat_map(|c|c.text.split_whitespace()).map(str::to_owned).collect::<Vec<_>>();
            prop_assert_eq!(actual.len(),expected.len());prop_assert_eq!(actual.into_iter().collect::<BTreeSet<_>>(),expected);
        }
    }
}

/// 同じ語が二つのセルにあるとき、一つだけ残しても照合成功にしない。
#[test]
fn source_text_validation_counts_repeated_cells_separately() {
    let cell = DocxTableCellModel {
        text: "same".into(),
        underlined: false,
        alignment: ParagraphAlignment::Left,
        vertical_alignment: None,
        grid_span: 2,
        vertical_merge: None,
        width_twips: None,
        borders: None,
    };
    let model = DocxTableModel {
        rows: vec![vec![cell]],
    };
    assert!(
        !model_preserves_grid_cell_texts(&model, &[vec!["same".into(), "same".into()]]),
        "one repeated word must not satisfy two original cell owners"
    );
}

/// 全体の文字数が同じでも、別のセルへ値を交換したモデルは拒否する。
#[test]
fn source_text_validation_checks_each_physical_owner() {
    let cell = |text: &str| DocxTableCellModel {
        text: text.into(),
        underlined: false,
        alignment: ParagraphAlignment::Left,
        vertical_alignment: None,
        grid_span: 1,
        vertical_merge: None,
        width_twips: None,
        borders: None,
    };
    let model = DocxTableModel {
        rows: vec![vec![cell("right"), cell("left")]],
    };
    assert!(
        !model_preserves_grid_cell_texts(&model, &[vec!["left".into(), "right".into()]]),
        "global text equality must not accept values moved to another source cell"
    );
}
