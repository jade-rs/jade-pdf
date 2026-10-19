//! PP-Structure 等が出力する HTML 表断片から `OcrTable` を構築する。
//!
//! 結合セル（`colspan` / `rowspan`）は矩形格子に展開して取り込む（`colspan` は先頭セルに
//! テキストを置き、右側は空文字。`rowspan` は下段セルを空にして占有する）。

use std::fmt::{Display, Formatter};

use jade_pdf2docx_ocr::OcrTable;
use scraper::{Html, Selector};

/// HTML 表の解析エラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlTableParseError {
    /// 文書に `<table>` が無い。
    NoTable,
    /// 表にデータ行が無い。
    EmptyTable,
    /// 行の列数が揃わない（展開後も不整合）。
    RaggedRows {
        /// 問題の行インデックス（0-based）。
        row_idx: usize,
        /// 期待列数。
        expected_cols: usize,
        /// 実際の列数。
        actual_cols: usize,
    },
    /// `rowspan` / `colspan` の数値が不正。
    InvalidSpanAttribute {
        /// 属性名。
        attr: &'static str,
        /// 生の値。
        raw: String,
    },
}

impl Display for HtmlTableParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HtmlTableParseError::NoTable => write!(f, "no <table> element in HTML fragment"),
            HtmlTableParseError::EmptyTable => write!(f, "table has no data rows"),
            HtmlTableParseError::RaggedRows {
                row_idx,
                expected_cols,
                actual_cols,
            } => write!(
                f,
                "table row {row_idx} has {actual_cols} cols after expansion, expected {expected_cols}"
            ),
            HtmlTableParseError::InvalidSpanAttribute { attr, raw } => {
                write!(f, "invalid {attr} attribute value: {raw}")
            }
        }
    }
}

impl std::error::Error for HtmlTableParseError {}

/// HTML 断片（または文書）から **最初の** `<table>` を解釈し `OcrTable` を構築する。
///
/// `#text` はセル内の連結テキスト（空白は正規化）を用いる。
pub fn ocr_table_from_html(html: &str) -> Result<OcrTable, HtmlTableParseError> {
    let document = Html::parse_fragment(html);
    let table_sel = Selector::parse("table").expect("selector table");
    let table = document
        .select(&table_sel)
        .next()
        .ok_or(HtmlTableParseError::NoTable)?;

    let tr_sel = Selector::parse("tr").expect("selector tr");
    let td_sel = Selector::parse("td, th").expect("selector td th");

    // 論理グリッド: `grid[r][c]` が `Some(_)` ならそのマスは占有済み。
    let mut grid: Vec<Vec<Option<String>>> = Vec::new();

    for (row_idx, tr) in table.select(&tr_sel).enumerate() {
        while grid.len() <= row_idx {
            grid.push(Vec::new());
        }

        let mut col_idx = 0usize;
        for cell in tr.select(&td_sel) {
            let colspan = parse_span_attr(cell.value().attr("colspan"), "colspan")?;
            let rowspan = parse_span_attr(cell.value().attr("rowspan"), "rowspan")?;
            let text = normalize_cell_text(&cell);

            // rowspan/colspan で占有済みのマスを飛ばす
            loop {
                ensure_row_width(&mut grid, row_idx, col_idx + 1);
                if grid[row_idx].get(col_idx).is_some_and(|c| c.is_some()) {
                    col_idx += 1;
                } else {
                    break;
                }
            }

            ensure_row_width(&mut grid, row_idx, col_idx + colspan.max(1));

            for rr in 0..rowspan {
                let r = row_idx + rr;
                if r >= grid.len() {
                    grid.push(Vec::new());
                }
                ensure_row_width(&mut grid, r, col_idx + colspan);
                for cc in 0..colspan {
                    let c = col_idx + cc;
                    let value = if rr == 0 && cc == 0 {
                        text.clone()
                    } else {
                        String::new()
                    };
                    if grid[r].get(c).is_some_and(|slot| slot.is_some()) {
                        return Err(HtmlTableParseError::RaggedRows {
                            row_idx,
                            expected_cols: col_idx + colspan,
                            actual_cols: c,
                        });
                    }
                    if grid[r].len() <= c {
                        grid[r].resize(c + 1, None);
                    }
                    grid[r][c] = Some(value);
                }
            }

            col_idx += colspan;
        }

        // 行末まで空セルが無いか（幅を揃える）
        let row_len = grid[row_idx].len();
        for r in row_idx..grid.len() {
            ensure_row_width(&mut grid, r, row_len);
        }
    }

    if grid.is_empty() {
        return Err(HtmlTableParseError::EmptyTable);
    }

    let expected = grid.iter().map(|row| row.len()).max().unwrap_or(0);
    if expected == 0 {
        return Err(HtmlTableParseError::EmptyTable);
    }

    let mut rows: Vec<Vec<String>> = Vec::with_capacity(grid.len());
    for (ri, row) in grid.iter().enumerate() {
        if row.len() != expected {
            return Err(HtmlTableParseError::RaggedRows {
                row_idx: ri,
                expected_cols: expected,
                actual_cols: row.len(),
            });
        }
        let mut out_row = Vec::with_capacity(expected);
        for cell in row {
            out_row.push(cell.clone().unwrap_or_default());
        }
        rows.push(out_row);
    }

    Ok(OcrTable::new(rows))
}

fn parse_span_attr(raw: Option<&str>, attr: &'static str) -> Result<usize, HtmlTableParseError> {
    match raw {
        None | Some("") => Ok(1),
        Some(s) => {
            let n: usize =
                s.trim()
                    .parse()
                    .map_err(|_| HtmlTableParseError::InvalidSpanAttribute {
                        attr,
                        raw: s.to_string(),
                    })?;
            if n == 0 {
                return Err(HtmlTableParseError::InvalidSpanAttribute {
                    attr,
                    raw: s.to_string(),
                });
            }
            Ok(n)
        }
    }
}

fn ensure_row_width(grid: &mut Vec<Vec<Option<String>>>, row_idx: usize, min_width: usize) {
    if grid.len() <= row_idx {
        grid.resize(row_idx + 1, Vec::new());
    }
    if grid[row_idx].len() < min_width {
        grid[row_idx].resize(min_width, None);
    }
}

fn normalize_cell_text(cell: &scraper::ElementRef<'_>) -> String {
    let s: String = cell.text().collect::<Vec<_>>().join(" ");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_2x2_table() {
        let html =
            r#"<table><tr><td>A1</td><td>B1</td></tr><tr><td>A2</td><td>B2</td></tr></table>"#;
        let t = ocr_table_from_html(html).expect("parse");
        assert_eq!(
            t.rows,
            vec![
                vec!["A1".to_string(), "B1".to_string()],
                vec!["A2".to_string(), "B2".to_string()],
            ]
        );
        assert!(t.validate().is_ok());
    }

    #[test]
    fn th_treated_as_cell() {
        let html = "<table><tr><th>X</th><th>Y</th></tr><tr><td>1</td><td>2</td></tr></table>";
        let t = ocr_table_from_html(html).expect("parse");
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0], vec!["X", "Y"]);
    }

    #[test]
    fn colspan_expands_first_cell_holds_text() {
        let html = "<table><tr><td colspan=\"2\">AB</td><td>C</td></tr></table>";
        let t = ocr_table_from_html(html).expect("parse");
        assert_eq!(t.rows[0], vec!["AB", "", "C"]);
    }

    #[test]
    fn rowspan_reserves_cells_below() {
        let html = "<table><tr><td rowspan=\"2\">M</td><td>a</td></tr><tr><td>b</td></tr></table>";
        let t = ocr_table_from_html(html).expect("parse");
        assert_eq!(
            t.rows,
            vec![vec!["M", "a".into()], vec!["".into(), "b".into()]]
        );
    }
}
