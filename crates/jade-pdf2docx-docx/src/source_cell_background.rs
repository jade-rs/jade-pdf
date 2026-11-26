//! 原本runの背後の塗りがセル全体を覆うときだけ、セル背景へ対応させる。
use super::{ParsedBounds, ParsedDocument, ParsedTextElement};

/// 確認済み罫線の内側半幅と整数gridの丸めだけを許容する。
fn covers(b: ParsedBounds, cell: ParsedBounds, edge_width: f32) -> bool {
    let tolerance = (edge_width * 0.5 + 0.51).clamp(0.51, 2.);
    b.x0 <= cell.x0 + tolerance
        && b.y0 <= cell.y0 + tolerance
        && b.x1 >= cell.x1 - tolerance
        && b.y1 >= cell.y1 - tolerance
}

/// 全文が同じ確認済み色を持ち、各矩形がセルを覆う場合だけ背景色を返す。
pub(super) fn color(
    texts: &[&ParsedTextElement],
    cell: ParsedBounds,
    edge_width: f32,
) -> Option<[u8; 3]> {
    let mut selected = texts.iter().filter(|text| !text.text.trim().is_empty());
    let first = selected.next()?.source_background?;
    if !covers(first.bounds, cell, edge_width) {
        return None;
    }
    selected
        .all(|text| {
            text.source_background
                .is_some_and(|b| b.rgb == first.rgb && covers(b.bounds, cell, edge_width))
        })
        .then_some(first.rgb)
}

/// 空セルでも、同じpageで検証した一枚の原本矩形が全体を覆う場合だけ色を返す。
pub(super) fn empty_color(
    parsed: &ParsedDocument,
    page: usize,
    cell: ParsedBounds,
    edge_width: f32,
) -> Option<[u8; 3]> {
    let mut backgrounds = parsed
        .text_elements
        .iter()
        .filter(|t| t.page == page)
        .filter_map(|t| t.source_background)
        .filter(|b| covers(b.bounds, cell, edge_width));
    let first = backgrounds.next()?;
    backgrounds.all(|b| b.rgb == first.rgb).then_some(first.rgb)
}
