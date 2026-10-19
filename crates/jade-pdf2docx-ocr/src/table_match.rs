//! セル境界ボックスと OCR テキストボックスを **幾何情報のみ** で対応付け、`OcrTable` を構築する。
//!
//! PP-Structure の `TableMatch` に相当する **最小版**（IoU 優先、次に包含、最後に中心距離）を提供する。
//!
//! セル割当に **Hungarian（最小費用）** を使う API も別途用意する（[`ocr_table_from_row_major_cells_and_texts_hungarian`]）。

use crate::hungarian::min_cost_assignment_square;
use crate::table::{OcrTable, OcrTableError};

/// 幾何マッチングが失敗したときのエラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableMatchError {
    /// 列数が 0。
    ZeroColumns,
    /// セル矩形が 1 つも無い。
    EmptyCells,
    /// テキストボックスが 1 つも無い（投影モード用）。
    EmptyTexts,
    /// `cell_rects.len()` が `cols` で割り切れない。
    CellsLenNotMultipleOfCols {
        /// セル数。
        cells_len: usize,
        /// 指定した列数。
        cols: usize,
    },
    /// 軸平行の罫線から有効な格子（少なくとも 1 セル）を復元できない。
    InvalidGridLines,
}

impl std::fmt::Display for TableMatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TableMatchError::ZeroColumns => write!(f, "cols must be greater than 0"),
            TableMatchError::EmptyCells => write!(f, "cell_rects must not be empty"),
            TableMatchError::EmptyTexts => write!(f, "text_boxes must not be empty"),
            TableMatchError::CellsLenNotMultipleOfCols { cells_len, cols } => write!(
                f,
                "cell_rects length {cells_len} is not a multiple of cols {cols}"
            ),
            TableMatchError::InvalidGridLines => write!(
                f,
                "axis-aligned line segments do not form a grid with at least one cell"
            ),
        }
    }
}

impl std::error::Error for TableMatchError {}

/// 軸平行矩形（画像座標。`x0 <= x1`, `y0 <= y1` を想定）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisRect {
    /// 左端の x。
    pub x0: f32,
    /// 上端の y。
    pub y0: f32,
    /// 右端の x。
    pub x1: f32,
    /// 下端の y。
    pub y1: f32,
}

impl AxisRect {
    /// 矩形を生成する（不正な順序は [`Self::normalize`] で正規化する）。
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// `x0 <= x1`, `y0 <= y1` となるよう並べ替える。
    pub fn normalize(self) -> Self {
        Self {
            x0: self.x0.min(self.x1),
            y0: self.y0.min(self.y1),
            x1: self.x0.max(self.x1),
            y1: self.y0.max(self.y1),
        }
    }

    /// 面積（負の幅・高さは 0 とみなす）。
    pub fn area(self) -> f32 {
        let w = (self.x1 - self.x0).max(0.0);
        let h = (self.y1 - self.y0).max(0.0);
        w * h
    }

    /// 交差面積。
    pub fn intersection_area(self, other: Self) -> f32 {
        let a = self.normalize();
        let b = other.normalize();
        let x0 = a.x0.max(b.x0);
        let y0 = a.y0.max(b.y0);
        let x1 = a.x1.min(b.x1);
        let y1 = a.y1.min(b.y1);
        if x0 >= x1 || y0 >= y1 {
            return 0.0;
        }
        (x1 - x0) * (y1 - y0)
    }

    /// IoU（交差 / 和集合）。両方面積 0 のときは 0。
    pub fn iou(self, other: Self) -> f32 {
        let a = self.normalize();
        let b = other.normalize();
        let inter = a.intersection_area(b);
        let u = a.area() + b.area() - inter;
        if u <= 0.0 {
            return 0.0;
        }
        inter / u
    }

    /// 中心座標。
    pub fn center(self) -> (f32, f32) {
        let a = self.normalize();
        ((a.x0 + a.x1) * 0.5, (a.y0 + a.y1) * 0.5)
    }

    /// 点 `(x, y)` が矩形に含まれるか（境界を含む）。
    pub fn contains_point(self, x: f32, y: f32) -> bool {
        let a = self.normalize();
        x >= a.x0 && x <= a.x1 && y >= a.y0 && y <= a.y1
    }
}

/// OCR テキスト 1 件とその境界ボックス。
#[derive(Debug, Clone, PartialEq)]
pub struct TextBox {
    /// テキスト領域。
    pub rect: AxisRect,
    /// 認識文字列。
    pub text: String,
}

impl TextBox {
    /// テキストボックスを生成する。
    pub fn new(rect: AxisRect, text: impl Into<String>) -> Self {
        Self {
            rect,
            text: text.into(),
        }
    }
}

/// セル矩形を **上→下・左→右** の読み順（中心 `(cy, cx)` の辞書順）で並べ替えたインデックス列を返す。
///
/// 行方向にそろった格子状のセルに対し、行メジャー順に近い順序を得る用途を想定する。
pub fn cell_indices_reading_order(cell_rects: &[AxisRect]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..cell_rects.len()).collect();
    idx.sort_by(|&a, &b| {
        let ra = cell_rects[a].normalize();
        let rb = cell_rects[b].normalize();
        let cay = (ra.y0 + ra.y1) * 0.5;
        let cby = (rb.y0 + rb.y1) * 0.5;
        let cax = (ra.x0 + ra.x1) * 0.5;
        let cbx = (rb.x0 + rb.x1) * 0.5;
        cay.total_cmp(&cby).then(cax.total_cmp(&cbx))
    });
    idx
}

/// セル bbox を持たず、**テキスト bbox のみ**から粗い格子表を推定する（研究用フォールバック）。
///
/// 手順:
/// 1. テキスト中心を **上→下・左→右** でソートし、隣接する `y` の差がしきい値を超えるところで **行** に分割（しきい値は高さ中央値に比例）。
/// 2. 全テキストの `x` 中心を昇順に並べ、隣接差がしきい値（幅中央値に比例）を超える位置で **列クラスタ** に分割し、各クラスタの平均を列中心とする。
/// 3. 各行の各テキストを **最も近い列中心** に割り当て、同一セルに複数入る場合は `x` 順に空白区切りで連結する。
///
/// 行方向は y クラスタ、列方向は **ページ全体の x クラスタ** により、行ごとの列数のばらつきより **列位置のずれ** に強い。
pub fn ocr_table_from_text_boxes_projection(
    texts: &[TextBox],
) -> Result<OcrTable, TableMatchError> {
    if texts.is_empty() {
        return Err(TableMatchError::EmptyTexts);
    }
    let n = texts.len();
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        let (ax, ay) = texts[a].rect.center();
        let (bx, by) = texts[b].rect.center();
        ay.total_cmp(&by).then(ax.total_cmp(&bx))
    });

    let mut sorted_h: Vec<f32> = texts
        .iter()
        .map(|t| {
            let r = t.rect.normalize();
            (r.y1 - r.y0).max(0.0)
        })
        .collect();
    sorted_h.sort_by(|a, b| a.total_cmp(b));
    let median_h = sorted_h[sorted_h.len() / 2];
    let sep_y = (median_h * 0.55).max(2.0);

    let mut sorted_w: Vec<f32> = texts
        .iter()
        .map(|t| {
            let r = t.rect.normalize();
            (r.x1 - r.x0).max(0.0)
        })
        .collect();
    sorted_w.sort_by(|a, b| a.total_cmp(b));
    let median_w = sorted_w[sorted_w.len() / 2];
    let sep_x = (median_w * 0.45).max(3.0);

    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut cur_row = vec![indices[0]];
    for i in 1..indices.len() {
        let prev_idx = indices[i - 1];
        let cur_idx = indices[i];
        let prev_cy = texts[prev_idx].rect.center().1;
        let cur_cy = texts[cur_idx].rect.center().1;
        if cur_cy - prev_cy > sep_y {
            cur_row.sort_by(|&a, &b| {
                texts[a]
                    .rect
                    .center()
                    .0
                    .total_cmp(&texts[b].rect.center().0)
            });
            rows.push(cur_row);
            cur_row = vec![cur_idx];
        } else {
            cur_row.push(cur_idx);
        }
    }
    cur_row.sort_by(|&a, &b| {
        texts[a]
            .rect
            .center()
            .0
            .total_cmp(&texts[b].rect.center().0)
    });
    rows.push(cur_row);

    let mut all_cx: Vec<f32> = texts.iter().map(|t| t.rect.center().0).collect();
    all_cx.sort_by(|a, b| a.total_cmp(b));
    let mut col_centers = cluster_sorted_1d_means(&all_cx, sep_x);
    col_centers = merge_adjacent_column_centers(col_centers, median_w);
    let cols = col_centers.len();
    if cols == 0 {
        return Err(TableMatchError::EmptyCells);
    }

    let mut out_rows: Vec<Vec<String>> = Vec::with_capacity(rows.len());
    for row in rows {
        let mut buckets: Vec<Vec<(f32, String)>> = (0..cols).map(|_| Vec::new()).collect();
        for &ti in &row {
            let cx = texts[ti].rect.center().0;
            let col = nearest_index(cx, &col_centers);
            let s = texts[ti].text.trim().to_string();
            if !s.is_empty() {
                buckets[col].push((cx, s));
            }
        }
        for b in &mut buckets {
            b.sort_by(|a, c| a.0.total_cmp(&c.0));
        }
        let line: Vec<String> = buckets
            .into_iter()
            .map(|b| b.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join(" "))
            .collect();
        out_rows.push(line);
    }

    let table = OcrTable::new(out_rows);
    table.validate().map_err(|e| match e {
        OcrTableError::EmptyTable => TableMatchError::EmptyTexts,
        OcrTableError::RaggedRows { .. } => {
            debug_assert!(false, "projection build produced ragged table");
            TableMatchError::EmptyCells
        }
        _ => TableMatchError::EmptyCells,
    })?;
    Ok(table)
}

/// 隣接する列中心が **同一列の行間ずれ** 程度なら 1 列にまとめる（`sep` による過分割の補正）。
///
/// しきい値は幅中央値に比例し、隣接列が十分離れているケースではマージしない。
fn merge_adjacent_column_centers(mut centers: Vec<f32>, median_w: f32) -> Vec<f32> {
    let thr = (median_w * 0.85).max(4.0);
    centers.sort_by(|a, b| a.total_cmp(b));
    let mut changed = true;
    while changed && centers.len() > 1 {
        changed = false;
        let mut i = 0;
        let mut next: Vec<f32> = Vec::with_capacity(centers.len());
        while i < centers.len() {
            if i + 1 < centers.len() && centers[i + 1] - centers[i] < thr {
                next.push((centers[i] + centers[i + 1]) * 0.5);
                i += 2;
                changed = true;
            } else {
                next.push(centers[i]);
                i += 1;
            }
        }
        centers = next;
    }
    centers
}

/// 昇順にソート済みのスカラー列を `sep` 超のギャップで分割し、各クラスタの **算術平均** を返す。
fn cluster_sorted_1d_means(sorted: &[f32], sep: f32) -> Vec<f32> {
    if sorted.is_empty() {
        return Vec::new();
    }
    let mut groups: Vec<Vec<f32>> = Vec::new();
    let mut cur = vec![sorted[0]];
    for i in 1..sorted.len() {
        if sorted[i] - sorted[i - 1] > sep {
            groups.push(cur);
            cur = vec![sorted[i]];
        } else {
            cur.push(sorted[i]);
        }
    }
    groups.push(cur);
    groups
        .into_iter()
        .map(|g| g.iter().copied().sum::<f32>() / g.len() as f32)
        .collect()
}

fn nearest_index(value: f32, centers: &[f32]) -> usize {
    centers
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (value - *a)
                .abs()
                .partial_cmp(&(value - *b).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// 未ソートのセル矩形とテキストボックスから `OcrTable` を構築する。
///
/// `cols` は列数。行数は `cell_rects.len() / cols`。セルは [`cell_indices_reading_order`] で行メジャー順に並べ替えてからマッチングする。
pub fn ocr_table_from_unsorted_cells_and_texts(
    cols: usize,
    cell_rects: &[AxisRect],
    text_boxes: &[TextBox],
) -> Result<OcrTable, TableMatchError> {
    if cols == 0 {
        return Err(TableMatchError::ZeroColumns);
    }
    if cell_rects.is_empty() {
        return Err(TableMatchError::EmptyCells);
    }
    if !cell_rects.len().is_multiple_of(cols) {
        return Err(TableMatchError::CellsLenNotMultipleOfCols {
            cells_len: cell_rects.len(),
            cols,
        });
    }
    let order = cell_indices_reading_order(cell_rects);
    let ordered: Vec<AxisRect> = order.iter().map(|&i| cell_rects[i]).collect();
    ocr_table_from_row_major_cells_and_texts(cols, &ordered, text_boxes)
}

/// 行メジャー順に並んだセル矩形（長さは列数の倍数）とテキストから `OcrTable` を構築する。
pub fn ocr_table_from_row_major_cells_and_texts(
    cols: usize,
    cell_rects_row_major: &[AxisRect],
    text_boxes: &[TextBox],
) -> Result<OcrTable, TableMatchError> {
    if cols == 0 {
        return Err(TableMatchError::ZeroColumns);
    }
    if cell_rects_row_major.is_empty() {
        return Err(TableMatchError::EmptyCells);
    }
    if !cell_rects_row_major.len().is_multiple_of(cols) {
        return Err(TableMatchError::CellsLenNotMultipleOfCols {
            cells_len: cell_rects_row_major.len(),
            cols,
        });
    }
    let rows = cell_rects_row_major.len() / cols;
    let mut grid: Vec<Vec<Vec<(f32, f32, String)>>> = (0..rows)
        .map(|_| (0..cols).map(|_| Vec::new()).collect())
        .collect();

    for tb in text_boxes {
        let (r, c) = best_cell_for_text(cell_rects_row_major, cols, tb);
        let (cx, cy) = tb.rect.center();
        grid[r][c].push((cx, cy, tb.text.trim().to_string()));
    }

    let mut out_rows: Vec<Vec<String>> = Vec::with_capacity(rows);
    for row in grid.iter_mut() {
        let mut line = Vec::with_capacity(cols);
        for cell in row.iter_mut() {
            let mut parts = std::mem::take(cell);
            parts.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
            let joined = parts
                .into_iter()
                .filter(|p| !p.2.is_empty())
                .map(|p| p.2)
                .collect::<Vec<_>>()
                .join(" ");
            line.push(joined);
        }
        out_rows.push(line);
    }

    let table = OcrTable::new(out_rows);
    table.validate().map_err(|e| match e {
        OcrTableError::EmptyTable => TableMatchError::EmptyCells,
        OcrTableError::RaggedRows { .. } => {
            debug_assert!(false, "row-major build produced ragged table");
            TableMatchError::EmptyCells
        }
        _ => TableMatchError::EmptyCells,
    })?;
    Ok(table)
}

/// 行メジャーなセル矩形とテキストを **一対一（最大 `max(texts, cells)` 角の Hungarian）** で割り当て、`OcrTable` を構築する。
///
/// 費用は `1 - IoU(text, cell)`。テキスト数とセル数が異なるときはダミー行・列で正方化し、余剰テキストは「捨て」列、余剰セルはダミー行が埋める（費用 0）。
pub fn ocr_table_from_row_major_cells_and_texts_hungarian(
    cols: usize,
    cell_rects_row_major: &[AxisRect],
    text_boxes: &[TextBox],
) -> Result<OcrTable, TableMatchError> {
    if cols == 0 {
        return Err(TableMatchError::ZeroColumns);
    }
    if cell_rects_row_major.is_empty() {
        return Err(TableMatchError::EmptyCells);
    }
    if !cell_rects_row_major.len().is_multiple_of(cols) {
        return Err(TableMatchError::CellsLenNotMultipleOfCols {
            cells_len: cell_rects_row_major.len(),
            cols,
        });
    }
    let rows = cell_rects_row_major.len() / cols;
    let nc = cell_rects_row_major.len();
    let t = text_boxes.len();

    if t == 0 {
        let out_rows: Vec<Vec<String>> = (0..rows)
            .map(|_| (0..cols).map(|_| String::new()).collect())
            .collect();
        let table = OcrTable::new(out_rows);
        table.validate().map_err(|e| match e {
            OcrTableError::EmptyTable => TableMatchError::EmptyCells,
            OcrTableError::RaggedRows { .. } => TableMatchError::EmptyCells,
            _ => TableMatchError::EmptyCells,
        })?;
        return Ok(table);
    }

    let s = t.max(nc);
    let mut cost = vec![vec![1.0f32; s]; s];
    for (i, row) in cost.iter_mut().enumerate().take(t) {
        for (j, slot) in row.iter_mut().enumerate().take(nc) {
            *slot = 1.0
                - text_boxes[i]
                    .rect
                    .normalize()
                    .iou(cell_rects_row_major[j].normalize());
        }
    }
    for row in cost.iter_mut().take(t) {
        for slot in row.iter_mut().skip(nc) {
            *slot = 1.0;
        }
    }
    for row in cost.iter_mut().skip(t) {
        for slot in row.iter_mut().take(nc) {
            *slot = 0.0;
        }
    }
    for row in cost.iter_mut().skip(t) {
        for slot in row.iter_mut().skip(nc) {
            *slot = 0.0;
        }
    }

    let assignment = min_cost_assignment_square(&cost);

    let mut grid: Vec<Vec<Vec<(f32, f32, String)>>> = (0..rows)
        .map(|_| (0..cols).map(|_| Vec::new()).collect())
        .collect();

    for i in 0..t {
        let j = assignment[i];
        if j < nc {
            let r = j / cols;
            let c = j % cols;
            let (cx, cy) = text_boxes[i].rect.center();
            let st = text_boxes[i].text.trim().to_string();
            if !st.is_empty() {
                grid[r][c].push((cx, cy, st));
            }
        }
    }

    let mut out_rows: Vec<Vec<String>> = Vec::with_capacity(rows);
    for row in grid.iter_mut() {
        let mut line = Vec::with_capacity(cols);
        for cell in row.iter_mut() {
            let mut parts = std::mem::take(cell);
            parts.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
            let joined = parts
                .into_iter()
                .filter(|p| !p.2.is_empty())
                .map(|p| p.2)
                .collect::<Vec<_>>()
                .join(" ");
            line.push(joined);
        }
        out_rows.push(line);
    }

    let table = OcrTable::new(out_rows);
    table.validate().map_err(|e| match e {
        OcrTableError::EmptyTable => TableMatchError::EmptyCells,
        OcrTableError::RaggedRows { .. } => {
            debug_assert!(false, "hungarian build produced ragged table");
            TableMatchError::EmptyCells
        }
        _ => TableMatchError::EmptyCells,
    })?;
    Ok(table)
}

/// `text` に対し IoU が最大のセルを選ぶ。IoU が極小なら中心の包含、それも無ければ中心距離が最小のセル。
fn best_cell_for_text(
    cell_rects_row_major: &[AxisRect],
    cols: usize,
    text: &TextBox,
) -> (usize, usize) {
    let tr = text.rect.normalize();
    let mut best_i = 0usize;
    let mut best_iou = -1.0f32;
    for (i, cell) in cell_rects_row_major.iter().enumerate() {
        let iou = tr.iou(cell.normalize());
        if iou > best_iou {
            best_iou = iou;
            best_i = i;
        }
    }
    const IOU_EPS: f32 = 1e-5;
    if best_iou > IOU_EPS {
        let r = best_i / cols;
        let c = best_i % cols;
        return (r, c);
    }

    let (cx, cy) = tr.center();
    for (i, cell) in cell_rects_row_major.iter().enumerate() {
        if cell.normalize().contains_point(cx, cy) {
            let r = i / cols;
            let c = i % cols;
            return (r, c);
        }
    }

    let mut best_d2 = f32::MAX;
    best_i = 0;
    for (i, cell) in cell_rects_row_major.iter().enumerate() {
        let (ccx, ccy) = cell.normalize().center();
        let d2 = (ccx - cx).powi(2) + (ccy - cy).powi(2);
        if d2 < best_d2 {
            best_d2 = d2;
            best_i = i;
        }
    }
    let r = best_i / cols;
    let c = best_i % cols;
    (r, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iou_identical_is_one() {
        let r = AxisRect::new(0.0, 0.0, 10.0, 10.0);
        assert!((r.iou(r) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn iou_disjoint_is_zero() {
        let a = AxisRect::new(0.0, 0.0, 10.0, 10.0);
        let b = AxisRect::new(20.0, 0.0, 30.0, 10.0);
        assert_eq!(a.iou(b), 0.0);
    }

    #[test]
    fn row_major_2x2_four_texts() {
        let cells = vec![
            AxisRect::new(0.0, 0.0, 100.0, 100.0),
            AxisRect::new(100.0, 0.0, 200.0, 100.0),
            AxisRect::new(0.0, 100.0, 100.0, 200.0),
            AxisRect::new(100.0, 100.0, 200.0, 200.0),
        ];
        let texts = vec![
            TextBox::new(AxisRect::new(10.0, 10.0, 40.0, 40.0), "a"),
            TextBox::new(AxisRect::new(120.0, 10.0, 160.0, 40.0), "b"),
            TextBox::new(AxisRect::new(10.0, 120.0, 40.0, 150.0), "c"),
            TextBox::new(AxisRect::new(130.0, 130.0, 180.0, 160.0), "d"),
        ];
        let t = ocr_table_from_row_major_cells_and_texts(2, &cells, &texts).expect("ok");
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("a"), String::from("b")],
                vec![String::from("c"), String::from("d")],
            ]
        );
    }

    #[test]
    fn unsorted_cells_reordered_to_row_major() {
        // 入力順はバラバラだが、幾何位置は 2x2 格子。
        let cells = vec![
            AxisRect::new(100.0, 100.0, 200.0, 200.0),
            AxisRect::new(0.0, 0.0, 100.0, 100.0),
            AxisRect::new(100.0, 0.0, 200.0, 100.0),
            AxisRect::new(0.0, 100.0, 100.0, 200.0),
        ];
        let texts = vec![
            TextBox::new(AxisRect::new(40.0, 40.0, 60.0, 60.0), "A"),
            TextBox::new(AxisRect::new(140.0, 40.0, 160.0, 60.0), "B"),
            TextBox::new(AxisRect::new(40.0, 140.0, 60.0, 160.0), "C"),
            TextBox::new(AxisRect::new(140.0, 140.0, 160.0, 160.0), "D"),
        ];
        let t = ocr_table_from_unsorted_cells_and_texts(2, &cells, &texts).expect("ok");
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("A"), String::from("B")],
                vec![String::from("C"), String::from("D")],
            ]
        );
    }

    #[test]
    fn rejects_bad_col_count() {
        let cells = [AxisRect::new(0.0, 0.0, 1.0, 1.0); 3];
        let err = ocr_table_from_unsorted_cells_and_texts(2, &cells, &[]).unwrap_err();
        assert_eq!(
            err,
            TableMatchError::CellsLenNotMultipleOfCols {
                cells_len: 3,
                cols: 2
            }
        );
    }

    #[test]
    fn projection_empty_texts_rejected() {
        let err = ocr_table_from_text_boxes_projection(&[]).unwrap_err();
        assert_eq!(err, TableMatchError::EmptyTexts);
    }

    #[test]
    fn projection_2x2_matches_cell_based() {
        let texts = vec![
            TextBox::new(AxisRect::new(10.0, 10.0, 40.0, 40.0), "a"),
            TextBox::new(AxisRect::new(120.0, 10.0, 160.0, 40.0), "b"),
            TextBox::new(AxisRect::new(10.0, 120.0, 40.0, 150.0), "c"),
            TextBox::new(AxisRect::new(130.0, 130.0, 180.0, 160.0), "d"),
        ];
        let t = ocr_table_from_text_boxes_projection(&texts).expect("ok");
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("a"), String::from("b")],
                vec![String::from("c"), String::from("d")],
            ]
        );
    }

    #[test]
    fn projection_pads_shorter_row() {
        let texts = vec![
            TextBox::new(AxisRect::new(10.0, 10.0, 40.0, 40.0), "a"),
            TextBox::new(AxisRect::new(120.0, 10.0, 160.0, 40.0), "b"),
            TextBox::new(AxisRect::new(10.0, 120.0, 40.0, 150.0), "c"),
        ];
        let t = ocr_table_from_text_boxes_projection(&texts).expect("ok");
        assert_eq!(
            t.rows,
            vec![
                vec![String::from("a"), String::from("b")],
                vec![String::from("c"), String::new()],
            ]
        );
    }

    #[test]
    fn hungarian_2x2_matches_greedy_for_distinct_cells() {
        let cells = vec![
            AxisRect::new(0.0, 0.0, 100.0, 100.0),
            AxisRect::new(100.0, 0.0, 200.0, 100.0),
            AxisRect::new(0.0, 100.0, 100.0, 200.0),
            AxisRect::new(100.0, 100.0, 200.0, 200.0),
        ];
        let texts = vec![
            TextBox::new(AxisRect::new(10.0, 10.0, 40.0, 40.0), "a"),
            TextBox::new(AxisRect::new(120.0, 10.0, 160.0, 40.0), "b"),
            TextBox::new(AxisRect::new(10.0, 120.0, 40.0, 150.0), "c"),
            TextBox::new(AxisRect::new(130.0, 130.0, 180.0, 160.0), "d"),
        ];
        let g = ocr_table_from_row_major_cells_and_texts(2, &cells, &texts).expect("greedy");
        let h =
            ocr_table_from_row_major_cells_and_texts_hungarian(2, &cells, &texts).expect("hung");
        assert_eq!(g.rows, h.rows);
    }

    #[test]
    fn projection_aligns_shifted_second_column_via_global_x_clusters() {
        let texts = vec![
            TextBox::new(AxisRect::new(8.0, 0.0, 30.0, 20.0), "a"),
            TextBox::new(AxisRect::new(140.0, 0.0, 160.0, 20.0), "b"),
            TextBox::new(AxisRect::new(8.0, 50.0, 30.0, 70.0), "c"),
            TextBox::new(AxisRect::new(152.0, 50.0, 172.0, 70.0), "d"),
        ];
        let t = ocr_table_from_text_boxes_projection(&texts).expect("ok");
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.col_count(), Some(2));
        assert_eq!(t.rows[0][1], "b");
        assert_eq!(t.rows[1][1], "d");
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn iou_is_symmetric_and_bounded(
            ax0 in 0.0f32..50.0f32, ay0 in 0.0f32..50.0f32,
            ax1 in 50.0f32..100.0f32, ay1 in 50.0f32..100.0f32,
            bx0 in 0.0f32..50.0f32, by0 in 0.0f32..50.0f32,
            bx1 in 50.0f32..100.0f32, by1 in 50.0f32..100.0f32,
        ) {
            let a = AxisRect::new(ax0, ay0, ax1, ay1).normalize();
            let b = AxisRect::new(bx0, by0, bx1, by1).normalize();
            let iab = a.iou(b);
            let iba = b.iou(a);
            prop_assert!((iab - iba).abs() < 1e-4);
            prop_assert!((0.0..=1.0 + 1e-4).contains(&iab));
        }
    }
}
