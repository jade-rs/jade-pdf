//! 二部グラフの **最小費用** 割当（Hungarian / Kuhn–Munkres）。
//!
//! 入力は **正方** 費用行列 `n × n`（`f32`）。戻り値 `assignment[i] = j` は行 `i` が列 `j` に割り当てられる。
//! テキスト bbox とセル bbox の **一対一** 対応付けに [`crate::table_match::ocr_table_from_row_major_cells_and_texts_hungarian`] から利用する。

/// 最小費用を与える割当を返す。`cost[i][j]` が行 `i`・列 `j` の費用。
///
/// `cost` は正方かつ空でないこと。`NaN` を含む場合はパニックまたは不正な結果になりうる。
pub fn min_cost_assignment_square(cost: &[Vec<f32>]) -> Vec<usize> {
    let n = cost.len();
    assert!(!cost.is_empty(), "cost matrix must be non-empty");
    assert!(cost.iter().all(|r| r.len() == n), "cost must be square");

    // 1-based 配列（アルゴリズムの都合）
    let inf = f32::MAX / 4.0;
    let mut u = vec![0.0f32; n + 1];
    let mut v = vec![0.0f32; n + 1];
    let mut p = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];

    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![inf; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = inf;
            let mut j1 = 0usize;
            for j in 1..=n {
                if !used[j] {
                    let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }

    let mut result = vec![0usize; n];
    for j in 1..=n {
        if p[j] != 0 {
            result[p[j] - 1] = j - 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_by_two_prefers_small_diagonal() {
        let cost = vec![vec![1.0, 9.0], vec![9.0, 1.0]];
        let a = min_cost_assignment_square(&cost);
        assert_eq!(a, vec![0, 1]);
    }

    #[test]
    fn three_by_three_identity_cost() {
        let cost = vec![
            vec![0.0, 1.0, 2.0],
            vec![3.0, 0.0, 4.0],
            vec![5.0, 6.0, 0.0],
        ];
        let a = min_cost_assignment_square(&cost);
        assert_eq!(a, vec![0, 1, 2]);
    }
}
