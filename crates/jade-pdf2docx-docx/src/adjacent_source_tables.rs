//! 同じ上端の独立した罫線表だけを浮動配置し、一組分の高さを本文へ残す。
use super::{ParsedGraphicElement, ParsedPage, PositionedTable, StructuredBlock};
use docx_rs::{Paragraph, Table, TablePositionProperty};

/// 連続する横並び表の位置と、最後の表の後だけ確保する本文高。
#[derive(Clone, Copy)]
pub(super) struct Placement {
    /// 原本の左端をページ左端から測ったtwips。
    x: i32,
    /// 原本の上端をページ上端から測ったtwips。
    y: i32,
    /// 組の下端をPDF上向き座標で保持する。
    pub(super) bottom: f32,
    /// 組の末尾で一回だけ確保する高さ。
    pub(super) reserve: Option<i32>,
}

/// 位置と高さが、丸めても正のtwipsとして表現可能か確認する。
fn twips(value: f32) -> Option<i32> {
    let value = (f64::from(value) * 20.0).round();
    (value.is_finite() && (0.0..=f64::from(i32::MAX)).contains(&value)).then_some(value as i32)
}

/// 座標の整数化誤差一単位を含め、元の外辺全体が実線分で覆われるか検査する。
fn covered(low: f32, high: f32, intervals: impl Iterator<Item = (f32, f32)>) -> bool {
    if !low.is_finite() || !high.is_finite() || low >= high {
        return false;
    }
    let mut intervals: Vec<_> = intervals
        .filter(|(a, b)| a.is_finite() && b.is_finite() && a < b)
        .collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut reached = low;
    for (start, end) in intervals {
        if start - 1.0 > reached {
            return false;
        }
        reached = reached.max(end + 1.0);
        if reached >= high {
            return true;
        }
    }
    false
}

/// 編集可能な複数行・複数列の独立した罫線表だけを候補にする。
fn candidate<'a>(block: &'a StructuredBlock, page: &ParsedPage) -> Option<&'a PositionedTable> {
    let StructuredBlock::Table { table, .. } = block else {
        return None;
    };
    let b = table.bounds;
    (table.nested_tables.is_empty()
        && table.table.rows.len() >= 2
        && table.column_widths_twips.len() >= 2
        && [b.x0, b.x1, b.y0, b.y1].iter().all(|v| v.is_finite())
        && 0.0 <= b.x0
        && b.x0 < b.x1
        && b.x1 <= page.width
        && 0.0 <= b.y0
        && b.y0 < b.y1
        && b.y1 <= page.height)
        .then_some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 交差する横罫線の両側で分かれた外辺も、量子化誤差の範囲で連続して覆う。
    #[test]
    fn segmented_outer_edge_has_complete_coverage() {
        assert!(covered(0.0, 60.0, [(31.0, 60.0), (0.0, 29.0)].into_iter()));
        assert!(!covered(0.0, 60.0, [(0.0, 28.0), (31.0, 60.0)].into_iter()));
        assert!(!covered(0.0, 60.0, [(0.0, 29.0), (31.0, 58.0)].into_iter()));
    }
}

/// 原本の上下左右を覆う実罫線があり、文字揃えだけから作った表ではないと確認する。
fn framed(table: &PositionedTable, graphics: &[ParsedGraphicElement]) -> bool {
    let b = table.bounds;
    let valid = |g: &&ParsedGraphicElement| {
        let r = g.bounds;
        g.page == table.page
            && [r.x0, r.x1, r.y0, r.y1, g.stroke_width]
                .iter()
                .all(|v| v.is_finite())
            && g.stroke_width > 0.0
    };
    [b.y0, b.y1].iter().all(|y| {
        covered(
            b.x0,
            b.x1,
            graphics.iter().filter(valid).filter_map(|g| {
                let r = g.bounds;
                ((r.y0 - r.y1).abs() <= 0.01 && (r.y0 - y).abs() <= 1.0).then_some((r.x0, r.x1))
            }),
        )
    }) && [b.x0, b.x1].iter().all(|x| {
        covered(
            b.y0,
            b.y1,
            graphics.iter().filter(valid).filter_map(|g| {
                let r = g.bounds;
                ((r.x0 - r.x1).abs() <= 0.01 && (r.x0 - x).abs() <= 1.0).then_some((r.y0, r.y1))
            }),
        )
    })
}

/// 段組み・回転・交差する本文や画像を避け、横に離れた連続表の組だけを選ぶ。
pub(super) fn select(
    blocks: &[StructuredBlock],
    page: Option<&ParsedPage>,
    columned: bool,
    graphics: &[ParsedGraphicElement],
) -> Vec<Option<Placement>> {
    let mut result = vec![None; blocks.len()];
    let Some(page) = page.filter(|p| {
        !columned
            && p.rotation == 0
            && p.width.is_finite()
            && p.height.is_finite()
            && p.width > 0.0
            && p.height > 0.0
    }) else {
        return result;
    };
    let mut start = 0;
    while start < blocks.len() {
        let Some(first) = candidate(&blocks[start], page) else {
            start += 1;
            continue;
        };
        let top = first.bounds.y1;
        let end = start
            + blocks[start..]
                .iter()
                .take_while(|b| {
                    candidate(b, page).is_some_and(|t| (t.bounds.y1 - top).abs() <= 0.25)
                })
                .count();
        if end - start < 2 {
            start = end;
            continue;
        }
        let mut tables: Vec<_> = blocks[start..end]
            .iter()
            .map(|b| candidate(b, page).unwrap())
            .collect();
        tables.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
        let bottom = tables
            .iter()
            .map(|t| t.bounds.y0)
            .fold(f32::INFINITY, f32::min);
        let overlap = tables
            .windows(2)
            .any(|p| p[0].bounds.x1 + 2.0 > p[1].bounds.x0);
        let obstructed = blocks.iter().enumerate().any(|(i, b)| {
            if (start..end).contains(&i) {
                return false;
            }
            match b {
                StructuredBlock::Paragraph { line, .. } => {
                    line.top_y >= bottom && line.top_y <= top
                }
                StructuredBlock::Image { image, .. } => {
                    image.bounds.y0 < top && image.bounds.y1 > bottom
                }
                StructuredBlock::Table { table, .. } => {
                    table.bounds.y0 < top && table.bounds.y1 > bottom
                }
            }
        });
        if !overlap
            && !obstructed
            && tables.iter().all(|t| framed(t, graphics))
            && tables.iter().all(|t| twips(t.bounds.x0).is_some())
            && let (Some(y), Some(height)) = (twips(page.height - top), twips(top - bottom))
            && height > 0
        {
            for i in start..end {
                if let Some(x) = twips(candidate(&blocks[i], page).unwrap().bounds.x0) {
                    result[i] = Some(Placement {
                        x,
                        y,
                        bottom,
                        reserve: (i + 1 == end).then_some(height),
                    });
                }
            }
        }
        start = end;
    }
    result
}

impl Placement {
    /// セルと行列は変更せず、ページ原点から測った位置を付ける。
    pub(super) fn table(self, table: Table) -> Table {
        table.position(
            TablePositionProperty::new()
                .vertical_anchor("page")
                .horizontal_anchor("page")
                .position_x(self.x)
                .position_y(self.y)
                .left_from_text(0)
                .right_from_text(0),
        )
    }

    /// 後続本文や表が横へ回り込まないよう、一組の原本高を一度だけ確保する。
    pub(super) fn reservation(self) -> Option<Paragraph> {
        Some(
            Paragraph::new().line_spacing(super::native_line_pitch::spacing_twips(
                self.reserve?,
                0,
                0,
            )?),
        )
    }
}
