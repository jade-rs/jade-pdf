//! 横並び画像を含む非重複canvasを、本文flowから分離して原本page座標へ置く。
use super::*;

/// 正立page内の独立画像だけを返し、単画像・table・段組・本文重なりの旧経路を保つ。
pub(super) fn select(
    parsed: &ParsedDocument,
    page: usize,
    tables: &[PositionedTable],
    has_text_columns: bool,
) -> Vec<usize> {
    if has_text_columns || tables.iter().any(|table| table.page == page) {
        return Vec::new();
    }
    let Some(geometry) = parsed
        .pages
        .iter()
        .find(|source| source.page_number == page)
    else {
        return Vec::new();
    };
    let mut images = parsed
        .image_elements
        .iter()
        .enumerate()
        .filter(|(_, image)| image.page == page)
        .take(65)
        .collect::<Vec<_>>();
    if !(2..=64).contains(&images.len())
        || images
            .iter()
            .any(|(_, image)| image.floating || !image_anchor::verified_page_frame(image, geometry))
    {
        return Vec::new();
    }
    let text = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
        .take(4097)
        .collect::<Vec<_>>();
    if text.is_empty() || text.len() > 4096 || text.iter().any(|text| !positive(text.bounds)) {
        return Vec::new();
    }
    for (at, (_, a)) in images.iter().enumerate() {
        if images[at + 1..]
            .iter()
            .any(|(_, b)| overlap(a.bounds, b.bounds))
        {
            return Vec::new();
        }
    }
    // 本文と重なる画像自体は従来のflowへ残し、独立した横並び画像を妨げない。
    let original_count = images.len();
    images.retain(|(_, image)| !text.iter().any(|text| overlap(text.bounds, image.bounds)));
    let unresolved_flow = images.len() != original_count;
    if images.len() < 2 {
        return Vec::new();
    }
    let mut row_members = vec![false; images.len()];
    for (at, (_, a)) in images.iter().enumerate() {
        for (next, (_, b)) in images.iter().enumerate().skip(at + 1) {
            let across = a.bounds.x1 + 1. <= b.bounds.x0 || b.bounds.x1 + 1. <= a.bounds.x0;
            let common = a.bounds.y1.min(b.bounds.y1) - a.bounds.y0.max(b.bounds.y0);
            let smaller = (a.bounds.y1 - a.bounds.y0).min(b.bounds.y1 - b.bounds.y0);
            if across && common >= smaller * 0.5 {
                row_members[at] = true;
                row_members[next] = true;
            }
        }
    }
    if !row_members.iter().any(|member| *member) {
        return Vec::new();
    }
    images
        .into_iter()
        .enumerate()
        .filter_map(|(at, (index, _))| (!unresolved_flow || row_members[at]).then_some(index))
        .collect()
}

/// 非有限・反転・面積0の本文矩形を、重なりなしの根拠に使わない。
fn positive(bounds: ParsedBounds) -> bool {
    [bounds.x0, bounds.x1, bounds.y0, bounds.y1]
        .iter()
        .all(|x| x.is_finite())
        && bounds.x0 < bounds.x1
        && bounds.y0 < bounds.y1
}

/// 正面積の交差だけを重なりとして扱い、接する原本端を推測で広げない。
fn overlap(a: ParsedBounds, b: ParsedBounds) -> bool {
    a.x0.max(b.x0) < a.x1.min(b.x1) && a.y0.max(b.y0) < a.y1.min(b.y1)
}
