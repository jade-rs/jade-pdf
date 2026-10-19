//! 軸平行の罫線（線分）からセル矩形のグリッドを推定する。
//!
//! OCR メタの `lines`（各要素が `[[x,y],[x,y]]` の端点ペア）を想定し、
//! ほぼ水平・ほぼ垂直のセグメントから **格子線の位置** をクラスタリングし、
//! 隣接線で囲まれた矩形を行メジャー順の [`crate::table_match::AxisRect`] 列にする。

use crate::table_match::{AxisRect, TableMatchError};

/// 軸平行罫線（線分の配列）からセル矩形と列数を推定する。
///
/// - 各線分は **ほぼ水平**（`dy` が小さい）または **ほぼ垂直**（`dx` が小さい）のみを利用し、それ以外は無視する。
/// - 水平線から y 座標、垂直線から x 座標を抽出し、近接座標をクラスタ平均でまとめる。
/// - 隣接する x・y の組で囲まれた矩形を **行メジャー**（上段左→右、次行…）で並べる。
///
/// # 戻り値
///
/// - `Vec<AxisRect>`: 行メジャー順のセル。長さは `(x 本数 - 1) * (y 本数 - 1)`。
/// - `usize`: 列数（1 行あたりのセル数）。
pub fn cells_from_axis_aligned_line_segments(
    lines: &[[[f32; 2]; 2]],
) -> Result<(Vec<AxisRect>, usize), TableMatchError> {
    if lines.is_empty() {
        return Err(TableMatchError::InvalidGridLines);
    }

    let mut lens = Vec::with_capacity(lines.len());
    for seg in lines {
        let p0 = seg[0];
        let p1 = seg[1];
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len > 1e-4 {
            lens.push(len);
        }
    }
    if lens.is_empty() {
        return Err(TableMatchError::InvalidGridLines);
    }
    lens.sort_by(|a, b| a.total_cmp(b));
    let median_len = lens[lens.len() / 2];
    let axis_eps = (median_len * 0.02).max(1.5);

    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for seg in lines {
        let p0 = seg[0];
        let p1 = seg[1];
        let dx = (p1[0] - p0[0]).abs();
        let dy = (p1[1] - p0[1]).abs();
        if dy <= axis_eps && dx > axis_eps {
            ys.push((p0[1] + p1[1]) * 0.5);
        } else if dx <= axis_eps && dy > axis_eps {
            xs.push((p0[0] + p1[0]) * 0.5);
        }
    }

    if xs.len() < 2 || ys.len() < 2 {
        return Err(TableMatchError::InvalidGridLines);
    }

    xs.sort_by(|a, b| a.total_cmp(b));
    ys.sort_by(|a, b| a.total_cmp(b));

    let merge_eps = (median_len * 0.02).max(2.0);
    let xs = cluster_sorted_1d_means(&xs, merge_eps);
    let ys = cluster_sorted_1d_means(&ys, merge_eps);

    if xs.len() < 2 || ys.len() < 2 {
        return Err(TableMatchError::InvalidGridLines);
    }

    let ncols = xs.len() - 1;
    let nrows = ys.len() - 1;
    let mut cells = Vec::with_capacity(nrows * ncols);
    for j in 0..nrows {
        let y0 = ys[j];
        let y1 = ys[j + 1];
        let (ry0, ry1) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        for i in 0..ncols {
            let x0 = xs[i];
            let x1 = xs[i + 1];
            let (rx0, rx1) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
            cells.push(AxisRect::new(rx0, ry0, rx1, ry1));
        }
    }

    Ok((cells, ncols))
}

/// 昇順のスカラー列を `sep` 超のギャップで分割し、各クラスタの算術平均を返す。
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 2×2 セル相当の外周＋十字（3×3 本の水平・垂直線）。
    fn grid_lines_2x2() -> Vec<[[f32; 2]; 2]> {
        vec![
            [[0.0, 0.0], [200.0, 0.0]],
            [[0.0, 100.0], [200.0, 100.0]],
            [[0.0, 200.0], [200.0, 200.0]],
            [[0.0, 0.0], [0.0, 200.0]],
            [[100.0, 0.0], [100.0, 200.0]],
            [[200.0, 0.0], [200.0, 200.0]],
        ]
    }

    /// 3×3 セル相当（4×4 の格子線交点を結ぶ水平 4 本・垂直 4 本）。
    fn grid_lines_3x3() -> Vec<[[f32; 2]; 2]> {
        vec![
            [[0.0, 0.0], [300.0, 0.0]],
            [[0.0, 100.0], [300.0, 100.0]],
            [[0.0, 200.0], [300.0, 200.0]],
            [[0.0, 300.0], [300.0, 300.0]],
            [[0.0, 0.0], [0.0, 300.0]],
            [[100.0, 0.0], [100.0, 300.0]],
            [[200.0, 0.0], [200.0, 300.0]],
            [[300.0, 0.0], [300.0, 300.0]],
        ]
    }

    #[test]
    fn two_by_two_grid_from_lines() {
        let lines = grid_lines_2x2();
        let (cells, cols) = cells_from_axis_aligned_line_segments(&lines).expect("grid");
        assert_eq!(cols, 2);
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0].x0, 0.0);
        assert_eq!(cells[0].y0, 0.0);
        assert_eq!(cells[0].x1, 100.0);
        assert_eq!(cells[0].y1, 100.0);
        assert_eq!(cells[3].x0, 100.0);
        assert_eq!(cells[3].y0, 100.0);
        assert_eq!(cells[3].x1, 200.0);
        assert_eq!(cells[3].y1, 200.0);
    }

    #[test]
    fn three_by_three_grid_from_lines() {
        let lines = grid_lines_3x3();
        let (cells, cols) = cells_from_axis_aligned_line_segments(&lines).expect("grid");
        assert_eq!(cols, 3);
        assert_eq!(cells.len(), 9);
        assert_eq!(cells[0].x0, 0.0);
        assert_eq!(cells[0].y0, 0.0);
        assert_eq!(cells[0].x1, 100.0);
        assert_eq!(cells[0].y1, 100.0);
        assert_eq!(cells[8].x0, 200.0);
        assert_eq!(cells[8].y0, 200.0);
        assert_eq!(cells[8].x1, 300.0);
        assert_eq!(cells[8].y1, 300.0);
    }

    #[test]
    fn rejects_only_horizontal_lines() {
        let lines: Vec<[[f32; 2]; 2]> =
            vec![[[0.0, 0.0], [100.0, 0.0]], [[0.0, 50.0], [100.0, 50.0]]];
        let err = cells_from_axis_aligned_line_segments(&lines).unwrap_err();
        assert_eq!(err, TableMatchError::InvalidGridLines);
    }

    #[test]
    fn rejects_empty_lines() {
        let lines: Vec<[[f32; 2]; 2]> = vec![];
        assert_eq!(
            cells_from_axis_aligned_line_segments(&lines).unwrap_err(),
            TableMatchError::InvalidGridLines
        );
    }

    /// 全座標を `s` 倍する（相似拡大）。軸平行性は保たれる。
    fn scale_grid(lines: &[[[f32; 2]; 2]], s: f32) -> Vec<[[f32; 2]; 2]> {
        lines
            .iter()
            .map(|seg| {
                [
                    [seg[0][0] * s, seg[0][1] * s],
                    [seg[1][0] * s, seg[1][1] * s],
                ]
            })
            .collect()
    }

    proptest! {
        /// 2×2 格子罫線を正のスカラーで相似拡大しても、列数・セル数・行メジャー整合は不変。
        #[test]
        fn scaled_2x2_grid_invariants(s in 0.5f32..100.0f32) {
            let base = grid_lines_2x2();
            let scaled = scale_grid(&base, s);
            let (cells, cols) = cells_from_axis_aligned_line_segments(&scaled).expect("valid grid");
            prop_assert_eq!(cols, 2);
            prop_assert_eq!(cells.len(), 4);
            prop_assert_eq!(cells.len() % cols, 0);
            let nrows = cells.len() / cols;
            prop_assert_eq!(nrows * cols, cells.len());
            for c in &cells {
                let n = c.normalize();
                prop_assert!(n.x1 > n.x0 && n.y1 > n.y0, "positive area: {n:?}");
            }
        }

        /// 3×3 格子罫線を相似拡大しても列数 3・セル数 9 を維持する。
        #[test]
        fn scaled_3x3_grid_invariants(s in 0.5f32..50.0f32) {
            let base = grid_lines_3x3();
            let scaled = scale_grid(&base, s);
            let (cells, cols) = cells_from_axis_aligned_line_segments(&scaled).expect("valid grid");
            prop_assert_eq!(cols, 3);
            prop_assert_eq!(cells.len(), 9);
            prop_assert_eq!(cells.len() % cols, 0);
            for c in &cells {
                let n = c.normalize();
                prop_assert!(n.x1 > n.x0 && n.y1 > n.y0, "positive area: {n:?}");
            }
        }
    }
}
