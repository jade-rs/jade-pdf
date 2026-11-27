//! 全幅の横罫線と各行で共通する空隙から、縦罫線のない表の列を復元する。
use super::*;

/// 一列に属する本文の左右端。すべての行を併合しても隣列へはみ出せない。
#[derive(Clone)]
struct Column {
    /// 本文の最も左の座標。
    left: f32,
    /// 本文の最も右の座標。
    right: f32,
}

/// 複数の行で共通する本文群から、空セルを許しつつ字面を横切らない列境界を求める。
pub(super) fn columns(
    parsed: &ParsedDocument,
    page: usize,
    rules: &[&ParsedBounds],
    ys: &[i32],
) -> Option<Vec<i32>> {
    let [bottom, _, .., top] = ys else {
        return None;
    };
    let outer = rules.first()?;
    let (left, right) = (outer.x0, outer.x1);
    if !rules.iter().all(|rule| rule.x0 == left && rule.x1 == right) {
        return None;
    }

    let mut rows = vec![Vec::new(); ys.len() - 1];
    for text in parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
    {
        let b = normalized_bounds(text.bounds);
        if b.x1 <= left || b.x0 >= right || b.y1 <= *bottom as f32 || b.y0 >= *top as f32 {
            continue;
        }
        if ![b.x0, b.x1, b.y0, b.y1].iter().all(|v| v.is_finite())
            || b.x0 >= b.x1
            || b.y0 >= b.y1
            || b.x0 < left
            || b.x1 > right
        {
            return None;
        }
        let row = row_index_for_y(b.y0 / 2.0 + b.y1 / 2.0, ys)?;
        let band = ys.len() - 2 - row;
        if b.y0 < ys[band] as f32 || b.y1 > ys[band + 1] as f32 {
            return None;
        }
        rows[row].push(b);
    }

    let rows = rows.into_iter().map(group_columns).collect::<Vec<_>>();
    let count = rows.iter().map(Vec::len).max()?;
    if count < 2 {
        return None;
    }
    let mut full_rows = rows.iter().filter(|row| row.len() == count).peekable();
    let mut columns = full_rows.next()?.clone();
    full_rows.peek()?;
    for row in full_rows {
        for (column, source) in columns.iter_mut().zip(row) {
            column.left = column.left.min(source.left);
            column.right = column.right.max(source.right);
        }
    }

    let inset = columns[0].left - left;
    let mut xs = vec![left as i32];
    for pair in columns.windows(2) {
        let first = pair[0].right.ceil() as i32;
        // 既存の所有判定は左列へ1ptの許容差を持つ。次列の文字はその外に置く。
        let last = (pair[1].left - 1.0).floor() as i32;
        if first > last {
            return None;
        }
        xs.push(((pair[1].left - inset).round() as i32).clamp(first, last));
    }
    xs.push(right as i32);
    (xs.windows(2).all(|pair| pair[0] < pair[1])
        && rows.iter().all(|row| groups_fit_columns(row, &xs)))
    .then_some(xs)
}

/// 欠けたセルを補っても、一つの本文群が隣の列へはみ出さないことを確認する。
fn groups_fit_columns(row: &[Column], xs: &[i32]) -> bool {
    let mut prior = None;
    for group in row {
        let index = xs
            .partition_point(|&x| x as f32 <= group.left)
            .saturating_sub(1);
        let Some(&right) = xs.get(index + 1) else {
            return false;
        };
        if group.right > right as f32
            || (index > 0 && group.left < xs[index] as f32 + 1.0)
            || prior.is_some_and(|previous| previous >= index)
        {
            return false;
        }
        prior = Some(index);
    }
    true
}

/// 既存の反復列表と同じ間隔で、行内の断片を列ごとの外接範囲へまとめる。
fn group_columns(mut row: Vec<ParsedBounds>) -> Vec<Column> {
    row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let mut columns: Vec<Column> = Vec::new();
    for b in row {
        if let Some(column) = columns
            .last_mut()
            .filter(|column| b.x0 - column.right <= TEXT_CELL_GAP_POINTS)
        {
            column.right = column.right.max(b.x1);
        } else {
            columns.push(Column {
                left: b.x0,
                right: b.x1,
            });
        }
    }
    columns
}
