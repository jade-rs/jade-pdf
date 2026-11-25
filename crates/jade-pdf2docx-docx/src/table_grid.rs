//! 罫線格子から結合セルと各セル辺の罫線有無を求める。
//!
//! 格子の隣り合うマスの境界に罫線があるかを、その辺を覆う罫線区間の和で判定する。
//! 罫線のない境界で隣接するマスを 1 つのセルへまとめ（union-find）、
//! DOCX で表せない矩形でないまとまりは行ごとの横結合へ分ける。

/// 罫線 1 本（水平なら `at` が y、垂直なら x）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rule {
    /// 線の位置（水平線の y、垂直線の x）。
    pub(crate) at: f32,
    /// 線の始点（水平線の x0、垂直線の y0）。
    pub(crate) start: f32,
    /// 線の終点（水平線の x1、垂直線の y1）。
    pub(crate) end: f32,
}

/// 結合後のセル矩形（上から数えた行、左から数えた列、両端を含む）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellRect {
    /// 先頭行。
    pub(crate) row0: usize,
    /// 先頭列。
    pub(crate) col0: usize,
    /// 末尾行。
    pub(crate) row1: usize,
    /// 末尾列。
    pub(crate) col1: usize,
}

/// 線の位置ずれとして許す距離（pt）。
const POSITION_TOLERANCE: f32 = 1.5;
/// 線端の隙間として許す距離（pt）。
const END_TOLERANCE: f32 = 1.5;
/// 境界のこの割合以上が罫線で覆われていれば罫線ありとみなす。
const COVERED_RATIO: f32 = 0.6;

/// 位置 `at` 付近の罫線区間が `[from, to]` を覆う割合を返す。
pub(crate) fn coverage(rules: &[Rule], at: f32, from: f32, to: f32) -> f32 {
    let (from, to) = (from.min(to), from.max(to));
    let length = to - from;
    if length <= 0.0 {
        return 0.0;
    }
    let mut intervals = rules
        .iter()
        .filter(|rule| (rule.at - at).abs() <= POSITION_TOLERANCE)
        .map(|rule| {
            (
                rule.start.min(rule.end) - END_TOLERANCE,
                rule.start.max(rule.end) + END_TOLERANCE,
            )
        })
        .filter_map(|(start, end)| {
            let start = start.max(from);
            let end = end.min(to);
            (end > start).then_some((start, end))
        })
        .collect::<Vec<_>>();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut covered = 0.0;
    let mut current: Option<(f32, f32)> = None;
    for (start, end) in intervals {
        match current {
            Some((current_start, current_end)) if start <= current_end => {
                current = Some((current_start, current_end.max(end)));
            }
            Some((current_start, current_end)) => {
                covered += current_end - current_start;
                current = Some((start, end));
            }
            None => current = Some((start, end)),
        }
    }
    if let Some((start, end)) = current {
        covered += end - start;
    }
    covered / length
}

/// 境界が罫線で区切られているか判定する。
pub(crate) fn is_ruled(rules: &[Rule], at: f32, from: f32, to: f32) -> bool {
    coverage(rules, at, from, to) >= COVERED_RATIO
}

/// 罫線格子（`xs` 昇順、`ys` は PDF 座標の昇順）の結合セルを求める。
///
/// `has_text[row][col]` は上から数えたマスに本文があるか。縦罫線がなくても左右両側に本文がある
/// 境界は、別々の値を 1 セルへ潰さないよう区切りとして扱う。横罫線のない上下のマスは、
/// 1 つの高いセル内の複数行として結合する。
pub(crate) fn merged_cells(
    horizontal: &[Rule],
    vertical: &[Rule],
    xs: &[f32],
    ys: &[f32],
    has_text: &[Vec<bool>],
) -> Vec<CellRect> {
    let rows = ys.len().saturating_sub(1);
    let cols = xs.len().saturating_sub(1);
    if rows == 0 || cols == 0 {
        return Vec::new();
    }
    let top = |row: usize| ys[rows - row];
    let bottom = |row: usize| ys[rows - row - 1];
    let text = |row: usize, col: usize| {
        has_text
            .get(row)
            .and_then(|cells| cells.get(col))
            .copied()
            .unwrap_or(false)
    };
    let mut parent = (0..rows * cols).collect::<Vec<_>>();
    for row in 0..rows {
        for col in 0..cols {
            if col + 1 < cols
                && !is_ruled(vertical, xs[col + 1], bottom(row), top(row))
                && !(text(row, col) && text(row, col + 1))
            {
                union(&mut parent, row * cols + col, row * cols + col + 1);
            }
            if row + 1 < rows && !is_ruled(horizontal, bottom(row), xs[col], xs[col + 1]) {
                union(&mut parent, row * cols + col, (row + 1) * cols + col);
            }
        }
    }
    let mut groups = std::collections::BTreeMap::<usize, Vec<(usize, usize)>>::new();
    for row in 0..rows {
        for col in 0..cols {
            groups
                .entry(find(&mut parent, row * cols + col))
                .or_default()
                .push((row, col));
        }
    }
    let mut rects = Vec::new();
    for squares in groups.values() {
        let row0 = squares.iter().map(|square| square.0).min().unwrap_or(0);
        let row1 = squares.iter().map(|square| square.0).max().unwrap_or(0);
        let col0 = squares.iter().map(|square| square.1).min().unwrap_or(0);
        let col1 = squares.iter().map(|square| square.1).max().unwrap_or(0);
        if squares.len() == (row1 - row0 + 1) * (col1 - col0 + 1) {
            rects.push(CellRect {
                row0,
                col0,
                row1,
                col1,
            });
        } else {
            rects.extend(row_runs(squares));
        }
    }
    rects.sort_by_key(|rect| (rect.row0, rect.col0));
    rects
}

/// 矩形でないまとまりを、行ごとの連続したマスの横結合へ分ける。
fn row_runs(squares: &[(usize, usize)]) -> Vec<CellRect> {
    let mut sorted = squares.to_vec();
    sorted.sort();
    let mut runs: Vec<CellRect> = Vec::new();
    for (row, col) in sorted {
        match runs.last_mut() {
            Some(run) if run.row0 == row && run.col1 + 1 == col => run.col1 = col,
            _ => runs.push(CellRect {
                row0: row,
                col0: col,
                row1: row,
                col1: col,
            }),
        }
    }
    runs
}

/// union-find の根を返す。
fn find(parent: &mut [usize], index: usize) -> usize {
    let mut root = index;
    while parent[root] != root {
        root = parent[root];
    }
    let mut current = index;
    while parent[current] != root {
        let next = parent[current];
        parent[current] = root;
        current = next;
    }
    root
}

/// 2 つのまとまりを結合する。
fn union(parent: &mut [usize], a: usize, b: usize) {
    let a = find(parent, a);
    let b = find(parent, b);
    if a != b {
        parent[a.max(b)] = a.min(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全格子線を引いた rows x cols の罫線（各辺はマスごとに分割して描く）。
    fn segmented_grid(xs: &[f32], ys: &[f32]) -> (Vec<Rule>, Vec<Rule>) {
        let mut horizontal = Vec::new();
        for &y in ys {
            for pair in xs.windows(2) {
                horizontal.push(Rule {
                    at: y,
                    start: pair[0],
                    end: pair[1],
                });
            }
        }
        let mut vertical = Vec::new();
        for &x in xs {
            for pair in ys.windows(2) {
                vertical.push(Rule {
                    at: x,
                    start: pair[0],
                    end: pair[1],
                });
            }
        }
        (horizontal, vertical)
    }

    /// 罫線を取り除く。
    fn without(rules: &mut Vec<Rule>, at: f32, start: f32) {
        rules.retain(|rule| !(rule.at == at && rule.start == start));
    }

    /// 分割描画された罫線でも区間の和で覆われていれば区切りとみなす。
    #[test]
    fn segmented_rules_cover_the_edge() {
        let rules = [
            Rule {
                at: 10.0,
                start: 0.0,
                end: 50.0,
            },
            Rule {
                at: 10.0,
                start: 50.0,
                end: 100.0,
            },
        ];
        assert!(coverage(&rules, 10.0, 0.0, 100.0) > 0.99);
        assert!(coverage(&rules, 20.0, 0.0, 100.0) < 0.01);
    }

    /// 内側の縦横の罫線を欠いた 2x2 のまとまりを 1 つの矩形セルにする。
    #[test]
    fn missing_inner_rules_form_two_by_two_merge() {
        let xs = [0.0, 10.0, 20.0, 30.0];
        let ys = [0.0, 10.0, 20.0, 30.0];
        let (mut horizontal, mut vertical) = segmented_grid(&xs, &ys);
        // 上から 0-1 行、左から 1-2 列を結合する（内側の縦線 x=20 と横線 y=20 を消す）。
        without(&mut vertical, 20.0, 10.0);
        without(&mut vertical, 20.0, 20.0);
        without(&mut horizontal, 20.0, 10.0);
        without(&mut horizontal, 20.0, 20.0);
        let rects = merged_cells(&horizontal, &vertical, &xs, &ys, &[]);
        assert!(rects.contains(&CellRect {
            row0: 0,
            col0: 1,
            row1: 1,
            col1: 2
        }));
        assert_eq!(rects.len(), 6);
    }

    /// 縦罫線がなくても左右両側に本文があるマスは結合しない。
    #[test]
    fn text_on_both_sides_keeps_cells_apart() {
        let xs = [0.0, 10.0, 20.0];
        let ys = [0.0, 10.0];
        let (horizontal, mut vertical) = segmented_grid(&xs, &ys);
        without(&mut vertical, 10.0, 0.0);
        let rects = merged_cells(&horizontal, &vertical, &xs, &ys, &[vec![true, true]]);
        assert_eq!(rects.len(), 2);
        let rects = merged_cells(&horizontal, &vertical, &xs, &ys, &[vec![true, false]]);
        assert_eq!(
            rects,
            vec![CellRect {
                row0: 0,
                col0: 0,
                row1: 0,
                col1: 1
            }]
        );
    }

    /// L 字のまとまりは行ごとの横結合へ分ける。
    #[test]
    fn non_rectangular_group_splits_into_row_runs() {
        let xs = [0.0, 10.0, 20.0];
        let ys = [0.0, 10.0, 20.0];
        let (mut horizontal, mut vertical) = segmented_grid(&xs, &ys);
        // 上段の 2 マスと左下を結合する L 字（上段の中央縦線と左列の中央横線を消す）。
        without(&mut vertical, 10.0, 10.0);
        without(&mut horizontal, 10.0, 0.0);
        let rects = merged_cells(&horizontal, &vertical, &xs, &ys, &[]);
        assert!(rects.contains(&CellRect {
            row0: 0,
            col0: 0,
            row1: 0,
            col1: 1
        }));
        assert!(rects.contains(&CellRect {
            row0: 1,
            col0: 0,
            row1: 1,
            col1: 0
        }));
        assert!(rects.contains(&CellRect {
            row0: 1,
            col0: 1,
            row1: 1,
            col1: 1
        }));
    }
}
