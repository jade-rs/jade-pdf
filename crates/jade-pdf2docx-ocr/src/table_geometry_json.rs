//! Phase 2: セル bbox + テキスト bbox を記述した JSON から [`crate::OcrTable`] を構築する。
//!
//! JSON は [`parse_geometry_table_json`] のスキーマに従う。
//! セル `cells` が空でないとき、または **`cells` が空で `lines` からセルを推定したあと**、任意フィールド **`cell_assignment`**（既定: `greedy`）で、貪欲 IoU か Hungarian（`hungarian`）に切り替えられる。
//! **`cells` が空かつ `lines` が非空** のときは、軸平行罫線からセル格子を推定する（内部モジュール `table_lines_grid`）。`cells` と `lines` が両方非空のときは **`cells` を優先**し、`lines` は無視する。

use serde::Deserialize;

use crate::OcrTable;
use crate::table_lines_grid::cells_from_axis_aligned_line_segments;
use crate::table_match::{
    AxisRect, TableMatchError, TextBox, cell_indices_reading_order,
    ocr_table_from_row_major_cells_and_texts, ocr_table_from_row_major_cells_and_texts_hungarian,
    ocr_table_from_text_boxes_projection, ocr_table_from_unsorted_cells_and_texts,
};

/// 幾何表 JSON のパースまたはマッチング失敗。
#[derive(Debug)]
pub enum GeometryTableJsonError {
    /// `serde_json` の解釈エラー。
    Json(serde_json::Error),
    /// セル・テキストの幾何マッチに失敗。
    Match(TableMatchError),
    /// JSON の `cols`（0 以外）が罫線から推定した列数と一致しない。
    ColCountMismatchInJson {
        /// JSON に書かれた列数。
        json_cols: usize,
        /// 罫線から推定した列数。
        inferred_cols: usize,
    },
}

impl std::fmt::Display for GeometryTableJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeometryTableJsonError::Json(err) => write!(f, "geometry table json: {err}"),
            GeometryTableJsonError::Match(err) => write!(f, "geometry table match: {err}"),
            GeometryTableJsonError::ColCountMismatchInJson {
                json_cols,
                inferred_cols,
            } => write!(
                f,
                "geometry table json: cols ({json_cols}) does not match column count inferred from lines ({inferred_cols})"
            ),
        }
    }
}

impl std::error::Error for GeometryTableJsonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GeometryTableJsonError::Json(e) => Some(e),
            GeometryTableJsonError::Match(e) => Some(e),
            GeometryTableJsonError::ColCountMismatchInJson { .. } => None,
        }
    }
}

impl From<serde_json::Error> for GeometryTableJsonError {
    fn from(value: serde_json::Error) -> Self {
        GeometryTableJsonError::Json(value)
    }
}

impl From<TableMatchError> for GeometryTableJsonError {
    fn from(value: TableMatchError) -> Self {
        GeometryTableJsonError::Match(value)
    }
}

/// セル bbox があるときのテキスト→セル割当方式。
#[derive(Debug, Default, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum CellAssignmentJson {
    /// 各テキストが独立に IoU 最大セルを選ぶ（従来）。
    #[default]
    Greedy,
    /// `1 - IoU` を費用とした **一対一** 最小費用割当（Hungarian）。
    Hungarian,
}

/// JSON ルート（`docs` または CLI 仕様と同期すること）。
#[derive(Debug, Deserialize)]
struct GeometryTableJsonDoc {
    /// 列数（`cells` が空でないとき必須。`cells` が空のときは無視される）。
    #[serde(default)]
    cols: usize,
    /// `true` のとき `cells` は既に行メジャー順。`false`（既定）のときは [`crate::table_match::cell_indices_reading_order`] で整列する。
    #[serde(default)]
    cells_row_major: bool,
    /// セル bbox 列（長さは列数の倍数）。**空** のときは罫線または投影で表を推定する。
    #[serde(default)]
    cells: Vec<RectJson>,
    /// 軸平行罫線（各要素は `[[x0,y0],[x1,y1]]`）。**`cells` が空のときのみ**解釈され、セル矩形を推定する。`cells` が非空のときは無視される。
    #[serde(default)]
    lines: Vec<[[f32; 2]; 2]>,
    /// テキスト bbox + 文字列。
    texts: Vec<TextRectJson>,
    /// セルありのときの割当（省略時は `greedy`）。
    #[serde(default)]
    cell_assignment: CellAssignmentJson,
}

#[derive(Debug, Deserialize)]
struct RectJson {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

#[derive(Debug, Deserialize)]
struct TextRectJson {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    text: String,
}

/// UTF-8 JSON 文字列を [`OcrTable`] に変換する。
///
/// - `cells` を省略するか空配列にし、かつ **`lines` も空** のときは [`crate::table_match::ocr_table_from_text_boxes_projection`] による **投影モード**（`cols` / `cells_row_major` は無視）。
/// - `cells` が空で **`lines` が非空** のときは軸平行罫線からセル格子を推定する。`cols` を 0 以外で指定した場合は推定列数と一致必須。
///
/// セル bbox を直接渡すスキーマ例:
///
/// ```json
/// {
///   "cols": 2,
///   "cells_row_major": true,
///   "cells": [
///     { "x0": 0, "y0": 0, "x1": 100, "y1": 100 },
///     { "x0": 100, "y0": 0, "x1": 200, "y1": 100 },
///     { "x0": 0, "y0": 100, "x1": 100, "y1": 200 },
///     { "x0": 100, "y0": 100, "x1": 200, "y1": 200 }
///   ],
///   "texts": [
///     { "x0": 10, "y0": 10, "x1": 40, "y1": 40, "text": "a" },
///     { "x0": 110, "y0": 10, "x1": 160, "y1": 40, "text": "b" },
///     { "x0": 10, "y0": 110, "x1": 40, "y1": 150, "text": "c" },
///     { "x0": 130, "y0": 130, "x1": 180, "y1": 160, "text": "d" }
///   ]
/// }
/// ```
pub fn parse_geometry_table_json(slice: &str) -> Result<OcrTable, GeometryTableJsonError> {
    let doc: GeometryTableJsonDoc = serde_json::from_str(slice)?;
    let texts: Vec<TextBox> = doc
        .texts
        .iter()
        .map(|t| TextBox::new(AxisRect::new(t.x0, t.y0, t.x1, t.y1), t.text.clone()))
        .collect();

    if doc.cells.is_empty() {
        if !doc.lines.is_empty() {
            let (cells, inferred_cols) = cells_from_axis_aligned_line_segments(&doc.lines)?;
            if doc.cols != 0 && doc.cols != inferred_cols {
                return Err(GeometryTableJsonError::ColCountMismatchInJson {
                    json_cols: doc.cols,
                    inferred_cols,
                });
            }
            return build_table_from_cells(
                inferred_cols,
                doc.cells_row_major,
                doc.cell_assignment,
                cells,
                &texts,
            );
        }
        return Ok(ocr_table_from_text_boxes_projection(&texts)?);
    }

    if doc.cols == 0 {
        return Err(TableMatchError::ZeroColumns.into());
    }

    let cells: Vec<AxisRect> = doc
        .cells
        .iter()
        .map(|r| AxisRect::new(r.x0, r.y0, r.x1, r.y1))
        .collect();

    build_table_from_cells(
        doc.cols,
        doc.cells_row_major,
        doc.cell_assignment,
        cells,
        &texts,
    )
}

/// `cells` 矩形列とテキストから [`OcrTable`] を構築する（[`CellAssignmentJson`] に従う）。
fn build_table_from_cells(
    cols: usize,
    cells_row_major: bool,
    assignment: CellAssignmentJson,
    cells: Vec<AxisRect>,
    texts: &[TextBox],
) -> Result<OcrTable, GeometryTableJsonError> {
    match assignment {
        CellAssignmentJson::Greedy => {
            if cells_row_major {
                Ok(ocr_table_from_row_major_cells_and_texts(
                    cols, &cells, texts,
                )?)
            } else {
                Ok(ocr_table_from_unsorted_cells_and_texts(
                    cols, &cells, texts,
                )?)
            }
        }
        CellAssignmentJson::Hungarian => {
            let ordered = if cells_row_major {
                cells
            } else {
                let order = cell_indices_reading_order(&cells);
                order.iter().map(|&i| cells[i]).collect()
            };
            Ok(ocr_table_from_row_major_cells_and_texts_hungarian(
                cols, &ordered, texts,
            )?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_row_major_2x2_fixture_shape() {
        let json = r#"{
            "cols": 2,
            "cells_row_major": true,
            "cells": [
                { "x0": 0, "y0": 0, "x1": 100, "y1": 100 },
                { "x0": 100, "y0": 0, "x1": 200, "y1": 100 },
                { "x0": 0, "y0": 100, "x1": 100, "y1": 200 },
                { "x0": 100, "y0": 100, "x1": 200, "y1": 200 }
            ],
            "texts": [
                { "x0": 10, "y0": 10, "x1": 40, "y1": 40, "text": "a" },
                { "x0": 120, "y0": 10, "x1": 160, "y1": 40, "text": "b" },
                { "x0": 10, "y0": 120, "x1": 40, "y1": 150, "text": "c" },
                { "x0": 130, "y0": 130, "x1": 180, "y1": 160, "text": "d" }
            ]
        }"#;
        let t = parse_geometry_table_json(json).expect("parse");
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.col_count(), Some(2));
        assert!(t.validate().is_ok());
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("a"), String::from("b")],
                vec![String::from("c"), String::from("d")],
            ]
        );
    }

    #[test]
    fn parses_hungarian_cell_assignment_2x2() {
        let json = r#"{
            "cols": 2,
            "cells_row_major": true,
            "cell_assignment": "hungarian",
            "cells": [
                { "x0": 0, "y0": 0, "x1": 100, "y1": 100 },
                { "x0": 100, "y0": 0, "x1": 200, "y1": 100 },
                { "x0": 0, "y0": 100, "x1": 100, "y1": 200 },
                { "x0": 100, "y0": 100, "x1": 200, "y1": 200 }
            ],
            "texts": [
                { "x0": 10, "y0": 10, "x1": 40, "y1": 40, "text": "a" },
                { "x0": 120, "y0": 10, "x1": 160, "y1": 40, "text": "b" },
                { "x0": 10, "y0": 120, "x1": 40, "y1": 150, "text": "c" },
                { "x0": 130, "y0": 130, "x1": 180, "y1": 160, "text": "d" }
            ]
        }"#;
        let t = parse_geometry_table_json(json).expect("parse");
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("a"), String::from("b")],
                vec![String::from("c"), String::from("d")],
            ]
        );
    }

    #[test]
    fn parses_lines_grid_2x2_matches_explicit_cells() {
        let json = r#"{
            "lines": [
                [[0,0],[200,0]],
                [[0,100],[200,100]],
                [[0,200],[200,200]],
                [[0,0],[0,200]],
                [[100,0],[100,200]],
                [[200,0],[200,200]]
            ],
            "texts": [
                { "x0": 10, "y0": 10, "x1": 40, "y1": 40, "text": "a" },
                { "x0": 120, "y0": 10, "x1": 160, "y1": 40, "text": "b" },
                { "x0": 10, "y0": 120, "x1": 40, "y1": 150, "text": "c" },
                { "x0": 130, "y0": 130, "x1": 180, "y1": 160, "text": "d" }
            ]
        }"#;
        let t = parse_geometry_table_json(json).expect("parse");
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.col_count(), Some(2));
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("a"), String::from("b")],
                vec![String::from("c"), String::from("d")],
            ]
        );
    }

    #[test]
    fn parses_lines_grid_hungarian_2x2() {
        let json = r#"{
            "cell_assignment": "hungarian",
            "lines": [
                [[0,0],[200,0]],
                [[0,100],[200,100]],
                [[0,200],[200,200]],
                [[0,0],[0,200]],
                [[100,0],[100,200]],
                [[200,0],[200,200]]
            ],
            "texts": [
                { "x0": 10, "y0": 10, "x1": 40, "y1": 40, "text": "a" },
                { "x0": 120, "y0": 10, "x1": 160, "y1": 40, "text": "b" },
                { "x0": 10, "y0": 120, "x1": 40, "y1": 150, "text": "c" },
                { "x0": 130, "y0": 130, "x1": 180, "y1": 160, "text": "d" }
            ]
        }"#;
        let t = parse_geometry_table_json(json).expect("parse");
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("a"), String::from("b")],
                vec![String::from("c"), String::from("d")],
            ]
        );
    }

    #[test]
    fn parses_lines_grid_3x3_json() {
        let json = r#"{
            "lines": [
                [[0,0],[300,0]],
                [[0,100],[300,100]],
                [[0,200],[300,200]],
                [[0,300],[300,300]],
                [[0,0],[0,300]],
                [[100,0],[100,300]],
                [[200,0],[200,300]],
                [[300,0],[300,300]]
            ],
            "texts": [
                { "x0": 20, "y0": 20, "x1": 80, "y1": 80, "text": "1" },
                { "x0": 120, "y0": 20, "x1": 180, "y1": 80, "text": "2" },
                { "x0": 220, "y0": 20, "x1": 280, "y1": 80, "text": "3" },
                { "x0": 20, "y0": 120, "x1": 80, "y1": 180, "text": "4" },
                { "x0": 120, "y0": 120, "x1": 180, "y1": 180, "text": "5" },
                { "x0": 220, "y0": 120, "x1": 280, "y1": 180, "text": "6" },
                { "x0": 20, "y0": 220, "x1": 80, "y1": 280, "text": "7" },
                { "x0": 120, "y0": 220, "x1": 180, "y1": 280, "text": "8" },
                { "x0": 220, "y0": 220, "x1": 280, "y1": 280, "text": "9" }
            ]
        }"#;
        let t = parse_geometry_table_json(json).expect("parse");
        assert_eq!(t.row_count(), 3);
        assert_eq!(t.col_count(), Some(3));
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("1"), String::from("2"), String::from("3")],
                vec![String::from("4"), String::from("5"), String::from("6")],
                vec![String::from("7"), String::from("8"), String::from("9")],
            ]
        );
    }

    #[test]
    fn rejects_lines_when_json_cols_mismatch() {
        let json = r#"{
            "cols": 3,
            "lines": [
                [[0,0],[200,0]],
                [[0,100],[200,100]],
                [[0,200],[200,200]],
                [[0,0],[0,200]],
                [[100,0],[100,200]],
                [[200,0],[200,200]]
            ],
            "texts": [
                { "x0": 10, "y0": 10, "x1": 40, "y1": 40, "text": "a" }
            ]
        }"#;
        let err = parse_geometry_table_json(json).unwrap_err();
        match err {
            GeometryTableJsonError::ColCountMismatchInJson {
                json_cols,
                inferred_cols,
            } => {
                assert_eq!(json_cols, 3);
                assert_eq!(inferred_cols, 2);
            }
            _ => panic!("expected ColCountMismatchInJson, got {err:?}"),
        }
    }

    #[test]
    fn parses_projection_only_same_as_row_major_2x2() {
        let json = r#"{
            "texts": [
                { "x0": 10, "y0": 10, "x1": 40, "y1": 40, "text": "a" },
                { "x0": 120, "y0": 10, "x1": 160, "y1": 40, "text": "b" },
                { "x0": 10, "y0": 120, "x1": 40, "y1": 150, "text": "c" },
                { "x0": 130, "y0": 130, "x1": 180, "y1": 160, "text": "d" }
            ]
        }"#;
        let t = parse_geometry_table_json(json).expect("parse");
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.col_count(), Some(2));
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("a"), String::from("b")],
                vec![String::from("c"), String::from("d")],
            ]
        );
    }
}
