//! ページを縦に区切る長い罫線（段の区切り線）で、本文の段を分ける。
//!
//! 官報のように段を縦罫線で区切るページでは、同じ高さの左右の行を交互に読むと
//! 本文が混ざる。区切り線より左の段を先に、右の段を後に読む。

use super::*;

/// 段の区切りとみなす縦罫線の、ページの高さに対する最小の長さ。
const MIN_RULE_HEIGHT_RATIO: f32 = 0.6;
/// ページの左右端からこの割合の内側にある罫線だけを区切りとみなす（用紙の端の線を除く）。
/// 外枠の外に縦書きの見出しを置くページでは、外枠も見出しと本文の区切りになる。
const EDGE_MARGIN_RATIO: f32 = 0.05;
/// 近い区切り線（二重線・三重線）をまとめる距離（pt）。
const RULE_CLUSTER_PT: f32 = 25.0;
/// 区切りの両側に必要な文字片の数。
const MIN_TEXTS_PER_SIDE: usize = 5;

/// ページの段の区切りの x 座標（左から順）を返す。表の中の罫線は区切りにしない。
pub(super) fn column_band_edges(
    parsed: &ParsedDocument,
    page: usize,
    tables: &[PositionedTable],
) -> Vec<f32> {
    let Some(parsed_page) = parsed
        .pages
        .iter()
        .find(|candidate| candidate.page_number == page)
    else {
        return Vec::new();
    };
    let (width, height) = parsed_page_display_size_points(parsed_page);
    let mut xs = parsed
        .graphics
        .iter()
        .filter(|graphic| graphic.page == page)
        .map(|graphic| normalized_bounds(graphic.bounds))
        .filter(|bounds| is_vertical_line(bounds))
        .filter(|bounds| bounds.y1 - bounds.y0 >= MIN_RULE_HEIGHT_RATIO * height)
        .filter(|bounds| {
            bounds.x0 > EDGE_MARGIN_RATIO * width && bounds.x0 < (1.0 - EDGE_MARGIN_RATIO) * width
        })
        .filter(|bounds| {
            !tables.iter().any(|table| {
                let area = normalized_bounds(table.bounds);
                table.page == page
                    && bounds.x0 > area.x0 - 2.0
                    && bounds.x0 < area.x1 + 2.0
                    && bounds.y0 < area.y1
                    && bounds.y1 > area.y0
            })
        })
        .map(|bounds| bounds.x0)
        .collect::<Vec<_>>();
    xs.sort_by(f32::total_cmp);
    let mut edges: Vec<Vec<f32>> = Vec::new();
    for x in xs {
        match edges.last_mut() {
            Some(group) if x - group[group.len() - 1] <= RULE_CLUSTER_PT => group.push(x),
            _ => edges.push(vec![x]),
        }
    }
    edges
        .into_iter()
        .map(|group| group.iter().sum::<f32>() / group.len() as f32)
        .filter(|edge| {
            let (mut left, mut right) = (0usize, 0usize);
            for text in parsed.text_elements.iter().filter(|text| text.page == page) {
                let bounds = normalized_bounds(text.bounds);
                if bounds.x1 <= *edge {
                    left += 1;
                } else if bounds.x0 >= *edge {
                    right += 1;
                }
            }
            left >= MIN_TEXTS_PER_SIDE && right >= MIN_TEXTS_PER_SIDE
        })
        .collect()
}

/// x 座標が属する段の番号を返す。
pub(super) fn band_of(x: f32, edges: &[f32]) -> usize {
    edges.iter().filter(|edge| x > **edge).count()
}

/// 行の中心が属する段の番号を返す。
pub(super) fn line_band(line: &StructuredTextLine, edges: &[f32]) -> usize {
    band_of((line.left_x + line.right_x) / 2.0, edges)
}

/// 行を段の順（同じ段では上から下）に並べ替える。区切りがなければそのまま返す。
///
/// ページ端の幅の狭い段（余白に置いた縦書きの見出しなど）は本文の後へ回す。
pub(super) fn order_lines_by_band(
    mut lines: Vec<StructuredTextLine>,
    edges: &[f32],
    page_width: f32,
) -> Vec<StructuredTextLine> {
    if edges.is_empty() {
        return lines;
    }
    lines.sort_by(|a, b| {
        band_rank(line_band(a, edges), edges, page_width)
            .cmp(&band_rank(line_band(b, edges), edges, page_width))
            .then(b.top_y.total_cmp(&a.top_y))
            .then(a.left_x.total_cmp(&b.left_x))
    });
    lines
}

/// 段を読む順の順位（ページ端の幅の狭い段は最後）を返す。
fn band_rank(band: usize, edges: &[f32], page_width: f32) -> (bool, usize) {
    let margin = 0.1 * page_width;
    let last = edges.len();
    let is_margin = last > 0
        && ((band == 0 && edges[0] < margin)
            || (band == last && edges[last - 1] > page_width - margin));
    (is_margin, band)
}

/// 表や画像を、同じ段の行の流れの中で上端の位置に差し込む順序番号を返す。
pub(super) fn insertion_index_in_band(
    lines: &[StructuredTextLine],
    top_y: f32,
    center_x: f32,
    edges: &[f32],
    page_width: f32,
) -> usize {
    if edges.is_empty() {
        return block_insertion_order_index(lines, top_y);
    }
    let rank = band_rank(band_of(center_x, edges), edges, page_width);
    lines
        .iter()
        .position(|line| {
            let line_rank = band_rank(line_band(line, edges), edges, page_width);
            line_rank > rank || (line_rank == rank && line.top_y < top_y)
        })
        .map(|index| index.saturating_mul(2))
        .unwrap_or_else(|| lines.len().saturating_mul(2))
}

/// 4 辺の罫線で囲まれた枠（50pt 四方以上）を、ページの罫線から探す。
pub(super) fn ruled_frames(parsed: &ParsedDocument, page: usize) -> Vec<ParsedBounds> {
    let page_graphics = parsed
        .graphics
        .iter()
        .filter(|graphic| graphic.page == page)
        .cloned()
        .collect::<Vec<_>>();
    let (horizontal, vertical) = grid_rules_from_graphics(&page_graphics);
    let mut frames = Vec::new();
    for (index, left) in vertical.iter().enumerate() {
        for right in vertical.iter().skip(index + 1) {
            let (left, right) = if left.at <= right.at {
                (left, right)
            } else {
                (right, left)
            };
            if right.at - left.at < 50.0 {
                continue;
            }
            let bottom = left.start.max(right.start);
            let top = left.end.min(right.end);
            if top - bottom < 50.0
                || (left.start - right.start).abs() > 2.0
                || (left.end - right.end).abs() > 2.0
            {
                continue;
            }
            let spans = |y: f32| table_grid::coverage(&horizontal, y, left.at, right.at) >= 0.95;
            if spans(bottom) && spans(top) {
                frames.push(ParsedBounds {
                    x0: left.at,
                    y0: bottom,
                    x1: right.at,
                    y1: top,
                });
            }
        }
    }
    frames
}

/// 点を囲む最も小さい枠の番号を返す。
pub(super) fn frame_of(x: f32, y: f32, frames: &[ParsedBounds]) -> Option<usize> {
    frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| x > frame.x0 && x < frame.x1 && y > frame.y0 && y < frame.y1)
        .min_by(|a, b| {
            let area = |frame: &ParsedBounds| (frame.x1 - frame.x0) * (frame.y1 - frame.y0);
            area(a.1).total_cmp(&area(b.1))
        })
        .map(|(index, _)| index)
}

/// ページ端の幅の狭い段（余白の縦書きの見出し）の行を、読む順に 1 行へまとめる。
///
/// 縦書きの見出しは 1 文字から数文字ずつの行に分かれて抽出されるため、別々の段落にすると
/// 読めない。回転したページの見出しは下から上、それ以外は上から下へ読む。
pub(super) fn merge_margin_band_lines(
    lines: Vec<StructuredTextLine>,
    edges: &[f32],
    page_width: f32,
    bottom_to_top: bool,
) -> Vec<StructuredTextLine> {
    if edges.is_empty() {
        return lines;
    }
    let (mut body, margin): (Vec<_>, Vec<_>) = lines
        .into_iter()
        .partition(|line| !band_rank(line_band(line, edges), edges, page_width).0);
    if margin.len() < 2 {
        body.extend(margin);
        return body;
    }
    // 同じ文字を含む長い行に重なる短い行は、同じ字を二重に拾ったものとして除く。
    let duplicates = margin
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            let text = line.text.trim();
            // 数字だけの行（日付の数など）は短くて他の行にも現れやすいので残す。
            !text.is_empty()
                && !text
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ('０'..='９').contains(&ch))
                && margin.iter().enumerate().any(|(other_index, other)| {
                    other_index != *index
                        && other.text.trim().chars().count() > text.chars().count()
                        && other.text.contains(text)
                        && line_band(other, edges) == line_band(line, edges)
                })
        })
        .map(|(index, _)| index)
        .collect::<BTreeSet<_>>();
    let margin = margin
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !duplicates.contains(index))
        .map(|(_, line)| line)
        .collect::<Vec<_>>();
    let mut margin = margin;
    margin.sort_by(|a, b| {
        line_band(a, edges)
            .cmp(&line_band(b, edges))
            .then(if bottom_to_top {
                a.top_y.total_cmp(&b.top_y)
            } else {
                b.top_y.total_cmp(&a.top_y)
            })
    });
    let mut merged: Vec<StructuredTextLine> = Vec::new();
    for line in margin {
        match merged.last_mut() {
            Some(last) if line_band(last, edges) == line_band(&line, edges) => {
                last.text.push_str(&line.text);
                last.runs.extend(line.runs);
                last.left_x = last.left_x.min(line.left_x);
                last.right_x = last.right_x.max(line.right_x);
                last.top_y = last.top_y.max(line.top_y);
            }
            _ => merged.push(line),
        }
    }
    body.extend(merged);
    body
}
