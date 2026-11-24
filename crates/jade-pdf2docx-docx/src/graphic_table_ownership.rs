//! 矩形の結合範囲ごとに原本の文字を照合し、同語の欠落や別セルへの移動を拒否する。
use super::*;

/// 各セルの開始grid列とモデル参照を保持する、重複しない表示行。
type Row<'a> = Vec<(usize, &'a DocxTableCellModel)>;

/// 結合の全物理行を開始セルへ一意に対応させ、原本と同じ非空白文字を確認する。
pub(super) fn preserves(model: &DocxTableModel, cells: &[Vec<String>]) -> bool {
    let Some(cols) = cells.first().map(Vec::len).filter(|c| *c > 0) else {
        return false;
    };
    if model.rows.len() != cells.len() || cells.iter().any(|r| r.len() != cols) {
        return false;
    }
    let Some(layout) = layout(model, cols) else {
        return false;
    };
    for (ri, row) in layout.iter().enumerate() {
        for &(col, cell) in row {
            if cell.vertical_merge == Some(DocxVerticalMerge::Continue) {
                let Some(previous) = ri.checked_sub(1).and_then(|r| at(&layout[r], col)) else {
                    return false;
                };
                if previous.grid_span != cell.grid_span
                    || previous.vertical_merge.is_none()
                    || !cell.text.is_empty()
                {
                    return false;
                }
                continue;
            }
            let mut end = ri + 1;
            if cell.vertical_merge == Some(DocxVerticalMerge::Restart) {
                while end < layout.len()
                    && at(&layout[end], col).is_some_and(|next| {
                        next.grid_span == cell.grid_span
                            && next.vertical_merge == Some(DocxVerticalMerge::Continue)
                    })
                {
                    end += 1;
                }
            }
            let source = cells[ri..end]
                .iter()
                .flat_map(|row| row[col..col + cell.grid_span].iter().map(String::as_str));
            if characters(source) != characters(std::iter::once(cell.text.as_str())) {
                return false;
            }
        }
    }
    true
}

/// 全行を同じ物理列数の互いに重ならない範囲へ展開し、範囲外の結合を拒否する。
fn layout(model: &DocxTableModel, cols: usize) -> Option<Vec<Row<'_>>> {
    model
        .rows
        .iter()
        .map(|row| {
            let mut col = 0_usize;
            let mut out = Vec::with_capacity(row.len());
            for cell in row {
                if cell.grid_span == 0 {
                    return None;
                }
                let end = col.checked_add(cell.grid_span)?;
                if end > cols {
                    return None;
                }
                out.push((col, cell));
                col = end;
            }
            (col == cols).then_some(out)
        })
        .collect()
}

/// 横結合の途中を開始点と取り違えず、指定列で始まるセルだけを探す。
fn at<'a>(row: &Row<'a>, col: usize) -> Option<&'a DocxTableCellModel> {
    row.binary_search_by_key(&col, |(start, _)| *start)
        .ok()
        .map(|i| row[i].1)
}

/// 空白の補間だけを許し、Unicode正規化・別字への置換・同語の重複削除は行わない。
fn characters<'a>(parts: impl IntoIterator<Item = &'a str>) -> BTreeMap<char, usize> {
    let mut counts = BTreeMap::new();
    for part in parts {
        for ch in part.chars().filter(|c| !c.is_whitespace()) {
            *counts.entry(ch).or_default() += 1;
        }
    }
    counts
}
