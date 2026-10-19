//! OCR 由来の表構造（セル格子）を表す型。
//!
//! PP-Structure 等の外部パイプラインからの取り込み先として利用する。

/// 表 IR の検証エラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrTableError {
    /// 行が 1 行も無い。
    EmptyTable,
    /// 行ごとの列数が一致しない（じゃばら表）。
    RaggedRows {
        /// 基準とした行インデックス（0-based）。
        row_idx: usize,
        /// 期待する列数。
        expected_cols: usize,
        /// 実際の列数。
        actual_cols: usize,
    },
    /// span 行列の行数または列数がセル行列と一致しない。
    RaggedSpans {
        /// 不一致が起きた行インデックス（0-based）。
        row_idx: usize,
        /// 期待する span 列数。
        expected_cols: usize,
        /// 実際の span 列数。
        actual_cols: usize,
    },
    /// span の行数または列数が 0。
    InvalidSpan {
        /// セルの行インデックス（0-based）。
        row_idx: usize,
        /// セルの列インデックス（0-based）。
        col_idx: usize,
        /// 指定された縦結合行数。
        row_span: usize,
        /// 指定された横結合列数。
        col_span: usize,
    },
    /// span が表の矩形範囲を越える。
    SpanOutOfBounds {
        /// セルの行インデックス（0-based）。
        row_idx: usize,
        /// セルの列インデックス（0-based）。
        col_idx: usize,
    },
    /// 複数の span が同じセルを覆っている。
    OverlappingSpan {
        /// 重複して覆われた行インデックス（0-based）。
        row_idx: usize,
        /// 重複して覆われた列インデックス（0-based）。
        col_idx: usize,
    },
}

impl std::fmt::Display for OcrTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrTableError::EmptyTable => write!(f, "ocr table must have at least one row"),
            OcrTableError::RaggedRows {
                row_idx,
                expected_cols,
                actual_cols,
            } => write!(
                f,
                "ocr table row {row_idx} has {actual_cols} cols, expected {expected_cols}"
            ),
            OcrTableError::RaggedSpans {
                row_idx,
                expected_cols,
                actual_cols,
            } => write!(
                f,
                "ocr table span row {row_idx} has {actual_cols} cols, expected {expected_cols}"
            ),
            OcrTableError::InvalidSpan {
                row_idx,
                col_idx,
                row_span,
                col_span,
            } => write!(
                f,
                "ocr table span at row {row_idx}, col {col_idx} is invalid ({row_span}x{col_span})"
            ),
            OcrTableError::SpanOutOfBounds { row_idx, col_idx } => write!(
                f,
                "ocr table span at row {row_idx}, col {col_idx} exceeds table bounds"
            ),
            OcrTableError::OverlappingSpan { row_idx, col_idx } => {
                write!(f, "ocr table spans overlap at row {row_idx}, col {col_idx}")
            }
        }
    }
}

impl std::error::Error for OcrTableError {}

/// OCR / 構造推論の結果を格納する矩形格子の表。
///
/// `rows[r][c]` が r 行 c 列のセル文字列。結合セルを扱う場合は
/// `spans[r][c]` に開始セルの結合幅を入れ、覆われるセルは通常の 1x1 span のままにする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrTable {
    /// 行の並び。各行は同じ長さのセル列であること。
    pub rows: Vec<Vec<String>>,
    /// 各セルの結合情報。`rows` と同じ矩形サイズを持つこと。
    pub spans: Vec<Vec<OcrCellSpan>>,
}

/// OCR 表セルの結合幅。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrCellSpan {
    /// 縦方向に結合する行数。結合しない場合は 1。
    pub row_span: usize,
    /// 横方向に結合する列数。結合しない場合は 1。
    pub col_span: usize,
}

impl OcrCellSpan {
    /// 結合しない 1x1 セルを返す。
    pub fn single() -> Self {
        Self {
            row_span: 1,
            col_span: 1,
        }
    }

    /// 指定した結合幅のセルを返す。
    pub fn new(row_span: usize, col_span: usize) -> Self {
        Self { row_span, col_span }
    }
}

impl OcrTable {
    /// 行データから表を構築する。
    pub fn new(rows: Vec<Vec<String>>) -> Self {
        let spans = rows
            .iter()
            .map(|row| vec![OcrCellSpan::single(); row.len()])
            .collect();
        Self { rows, spans }
    }

    /// 行データと結合情報から表を構築する。
    pub fn with_spans(rows: Vec<Vec<String>>, spans: Vec<Vec<OcrCellSpan>>) -> Self {
        Self { rows, spans }
    }

    /// 行数を返す。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 列数を返す（0 行のときは `None`）。
    pub fn col_count(&self) -> Option<usize> {
        self.rows.first().map(|r| r.len())
    }

    /// 矩形格子として整合しているか検証する。
    pub fn validate(&self) -> Result<(), OcrTableError> {
        if self.rows.is_empty() {
            return Err(OcrTableError::EmptyTable);
        }
        let expected = self.rows[0].len();
        for (i, row) in self.rows.iter().enumerate() {
            if row.len() != expected {
                return Err(OcrTableError::RaggedRows {
                    row_idx: i,
                    expected_cols: expected,
                    actual_cols: row.len(),
                });
            }
        }
        if self.spans.len() != self.rows.len() {
            return Err(OcrTableError::RaggedSpans {
                row_idx: self.spans.len().min(self.rows.len()),
                expected_cols: expected,
                actual_cols: 0,
            });
        }
        for (i, row) in self.spans.iter().enumerate() {
            if row.len() != expected {
                return Err(OcrTableError::RaggedSpans {
                    row_idx: i,
                    expected_cols: expected,
                    actual_cols: row.len(),
                });
            }
        }
        self.validate_spans(expected)
    }

    /// span 同士が矩形表の範囲内で重ならないか検証する。
    fn validate_spans(&self, cols: usize) -> Result<(), OcrTableError> {
        let rows = self.rows.len();
        let mut covered = vec![vec![None::<(usize, usize)>; cols]; rows];
        for row_idx in 0..rows {
            for col_idx in 0..cols {
                if covered[row_idx][col_idx].is_some() {
                    if self.spans[row_idx][col_idx] != OcrCellSpan::single() {
                        return Err(OcrTableError::OverlappingSpan { row_idx, col_idx });
                    }
                    continue;
                }
                let span = self.spans[row_idx][col_idx];
                if span.row_span == 0 || span.col_span == 0 {
                    return Err(OcrTableError::InvalidSpan {
                        row_idx,
                        col_idx,
                        row_span: span.row_span,
                        col_span: span.col_span,
                    });
                }
                if row_idx + span.row_span > rows || col_idx + span.col_span > cols {
                    return Err(OcrTableError::SpanOutOfBounds { row_idx, col_idx });
                }
                for (r, covered_row) in covered
                    .iter_mut()
                    .enumerate()
                    .skip(row_idx)
                    .take(span.row_span)
                {
                    for (c, covered_cell) in covered_row
                        .iter_mut()
                        .enumerate()
                        .skip(col_idx)
                        .take(span.col_span)
                    {
                        if covered_cell.is_some() {
                            return Err(OcrTableError::OverlappingSpan {
                                row_idx: r,
                                col_idx: c,
                            });
                        }
                        *covered_cell = Some((row_idx, col_idx));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn validate_accepts_uniform_rows() {
        let t = OcrTable::new(vec![
            vec!["a".into(), "b".into()],
            vec!["c".into(), "d".into()],
        ]);
        assert!(t.validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty() {
        let t = OcrTable::new(vec![]);
        assert!(matches!(t.validate(), Err(OcrTableError::EmptyTable)));
    }

    #[test]
    fn validate_rejects_ragged_rows() {
        let t = OcrTable::new(vec![vec!["a".into()], vec!["b".into(), "c".into()]]);
        assert!(matches!(
            t.validate(),
            Err(OcrTableError::RaggedRows { row_idx: 1, .. })
        ));
    }

    #[test]
    fn validate_accepts_colspan_and_rowspan() {
        let t = OcrTable::with_spans(
            vec![
                vec!["title".into(), "".into()],
                vec!["merged".into(), "b".into()],
                vec!["".into(), "d".into()],
            ],
            vec![
                vec![OcrCellSpan::new(1, 2), OcrCellSpan::single()],
                vec![OcrCellSpan::new(2, 1), OcrCellSpan::single()],
                vec![OcrCellSpan::single(), OcrCellSpan::single()],
            ],
        );
        assert!(t.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_span() {
        let t = OcrTable::with_spans(vec![vec!["a".into()]], vec![vec![OcrCellSpan::new(0, 1)]]);
        assert!(matches!(
            t.validate(),
            Err(OcrTableError::InvalidSpan { row_idx: 0, .. })
        ));
    }

    #[test]
    fn validate_rejects_overlapping_span() {
        let t = OcrTable::with_spans(
            vec![vec!["a".into(), "b".into()]],
            vec![vec![OcrCellSpan::new(1, 2), OcrCellSpan::new(1, 2)]],
        );
        assert!(matches!(
            t.validate(),
            Err(OcrTableError::OverlappingSpan { row_idx: 0, .. })
        ));
    }

    proptest! {
        #[test]
        fn validate_accepts_any_uniform_m_by_n(
            rows in 1usize..8usize,
            cols in 1usize..8usize,
            seed in any::<u64>()
        ) {
            let mut table_rows = Vec::new();
            let mut s = seed;
            for r in 0..rows {
                let mut row = Vec::new();
                for c in 0..cols {
                    s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                    row.push(format!("r{r}c{c}_{s}"));
                }
                table_rows.push(row);
            }
            let t = OcrTable::new(table_rows);
            prop_assert!(t.validate().is_ok());
            prop_assert_eq!(t.row_count(), rows);
            prop_assert_eq!(t.col_count(), Some(cols));
        }
    }
}
