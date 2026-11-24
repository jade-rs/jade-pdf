//! 欠けた辺を補わず、部分罫線を表現できる矩形のセル所有へ分割する。
use super::*;

/// 元の各行の矩形範囲を作り、同じ横幅を保つ縦結合だけを組み立てる。
pub(super) fn model(
    graphics: &[ParsedGraphicElement],
    xs: &[i32],
    ys: &[i32],
    cells: &[Vec<String>],
    underlines: &[Vec<bool>],
) -> Option<DocxTableModel> {
    if cells.is_empty()
        || cells.len().checked_add(1)? != ys.len()
        || xs.len() < 2
        || xs.windows(2).any(|p| p[0] >= p[1])
        || ys.windows(2).any(|p| p[0] >= p[1])
        || cells.iter().any(|r| r.len() + 1 != xs.len())
    {
        return None;
    }
    let size = table_border_size_for_pdf_rule(table_rule_stroke_width(graphics, xs, ys));
    let mut needed = false;
    let mut rows = Vec::with_capacity(cells.len());
    for (ri, row) in cells.iter().enumerate() {
        let band = ys.len() - 2 - ri;
        let bottom = ys[band];
        let top = ys[band + 1];
        let mut col = 0;
        let mut output = Vec::new();
        while col < row.len() {
            let mut span = 1;
            let mut text = row[col].clone();
            let mut underlined = underlines
                .get(ri)
                .and_then(|r| r.get(col))
                .copied()
                .unwrap_or(false);
            while col + span < row.len()
                && !has_vertical_boundary_in_band(graphics, xs[col + span], bottom, top)
                && should_merge_across_missing_vertical_boundary(row, col, span)
                && uniform_horizontal(graphics, xs, top, col, span + 1)
                && uniform_horizontal(graphics, xs, bottom, col, span + 1)
            {
                underlined |= underlines
                    .get(ri)
                    .and_then(|r| r.get(col + span))
                    .copied()
                    .unwrap_or(false);
                if !row[col + span].trim().is_empty() {
                    if !text.trim().is_empty() {
                        text.push(' ');
                    }
                    text.push_str(row[col + span].trim());
                }
                span += 1;
            }
            // 横結合の上下は全小区間で同じと検証済み。接した複数線分も一つの辺として保つ。
            let borders = DocxCellBorders {
                top: has_horizontal_boundary_in_column_band(graphics, top, xs[col], xs[col + 1]),
                bottom: has_horizontal_boundary_in_column_band(
                    graphics,
                    bottom,
                    xs[col],
                    xs[col + 1],
                ),
                left: has_vertical_boundary_in_band(graphics, xs[col], bottom, top),
                right: has_vertical_boundary_in_band(graphics, xs[col + span], bottom, top),
                size_eighths_pt: size,
            };
            needed |=
                span > 1 || !borders.top || !borders.bottom || !borders.left || !borders.right;
            output.push(DocxTableCellModel {
                text,
                underlined,
                alignment: ParagraphAlignment::Left,
                grid_span: span,
                vertical_merge: None,
                width_twips: None,
                borders: Some(borders),
            });
            col += span;
        }
        rows.push(output);
    }
    // 直前の行の同一範囲だけを接続する。T/L字の領域を単一の非矩形結合へ広げない。
    let mut previous = BTreeMap::<(usize, usize), (usize, usize)>::new();
    for ri in 0..rows.len() {
        let mut current = BTreeMap::new();
        let mut col = 0;
        for ci in 0..rows[ri].len() {
            let span = rows[ri][ci].grid_span;
            let key = (col, span);
            let owner = previous.get(&key).copied().filter(|(root, cell)| {
                let a = rows[*root][*cell].borders.expect("source borders assigned");
                let b = rows[ri][ci].borders.expect("source borders assigned");
                a.left == b.left
                    && a.right == b.right
                    && (col..col + span).all(|c| {
                        !has_horizontal_boundary_in_column_band(
                            graphics,
                            ys[ys.len() - 1 - ri],
                            xs[c],
                            xs[c + 1],
                        )
                    })
            });
            let owner = if let Some((root, cell)) = owner {
                let text = std::mem::take(&mut rows[ri][ci].text);
                let underlined = rows[ri][ci].underlined;
                let bottom = rows[ri][ci]
                    .borders
                    .expect("source borders assigned")
                    .bottom;
                rows[ri][ci].vertical_merge = Some(DocxVerticalMerge::Continue);
                let restart = &mut rows[root][cell];
                if !text.is_empty() {
                    if !restart.text.is_empty() {
                        restart.text.push('\n');
                    }
                    restart.text.push_str(&text);
                }
                restart.underlined |= underlined;
                restart.vertical_merge = Some(DocxVerticalMerge::Restart);
                restart
                    .borders
                    .as_mut()
                    .expect("source borders assigned")
                    .bottom = bottom;
                needed = true;
                (root, cell)
            } else {
                (ri, ci)
            };
            current.insert(key, owner);
            col += span;
        }
        previous = current;
    }
    needed.then_some(DocxTableModel { rows })
}

/// 結合後の辺に部分罫線を隠さないよう、元の各小区間の有無が同じことを確認する。
fn uniform_horizontal(
    graphics: &[ParsedGraphicElement],
    xs: &[i32],
    y: i32,
    col: usize,
    span: usize,
) -> bool {
    let first = has_horizontal_boundary_in_column_band(graphics, y, xs[col], xs[col + 1]);
    (col + 1..col + span)
        .all(|c| has_horizontal_boundary_in_column_band(graphics, y, xs[c], xs[c + 1]) == first)
}
