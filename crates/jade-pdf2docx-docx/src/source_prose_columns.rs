//! 表の所有やfooter抽出に先立ち、原本の長い散文列を照合する。
use super::*;

/// 左右各八行以上の長い散文がある原本では、表や未検証footerへ昇格しない。
pub(super) fn has_dense_rows(parsed: &ParsedDocument, page: &StructuredTextPage) -> bool {
    dense_column_count(parsed, page).is_some()
}

/// Wordで配置を確認した二列だけを、旧表推定から実段組へ戻す対象にする。
pub(super) fn has_two_dense_columns(parsed: &ParsedDocument, page: &StructuredTextPage) -> bool {
    dense_column_count(parsed, page) == Some(2)
        && source_anchor_count(parsed, page) == Some(2)
        && detected_text_columns_for_page(parsed, page, &[]).is_some()
}

/// 短い識別子列や同じ行へ結合された第三列も数え、二列の散文だけを取り出して誤認しない。
fn source_anchor_count(parsed: &ParsedDocument, page: &StructuredTextPage) -> Option<usize> {
    let geometry = parsed.pages.iter().find(|p| p.page_number == page.page)?;
    let width = parsed_page_display_size_points(geometry).0;
    if !width.is_finite() || width <= 0.0 {
        return None;
    }
    let mut lefts = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page.page && !text.text.trim().is_empty())
        .map(|text| {
            let bounds = text.bounds;
            (bounds.x0.is_finite()
                && bounds.x1.is_finite()
                && bounds.x0 >= 0.0
                && bounds.x0 < bounds.x1
                && bounds.x1 <= width)
                .then_some(bounds.x0)
        })
        .collect::<Option<Vec<_>>>()?;
    lefts.sort_by(f32::total_cmp);
    let gap = (width * 0.2).max(48.0);
    let mut first: Option<f32> = None;
    let mut count = 0;
    for left in lefts {
        if first.is_none_or(|start| left - start > gap) {
            first = Some(left);
            count += 1;
        }
    }
    Some(count)
}

/// 全ての原本散文列で八行以上を確認できた場合に、列数をそのまま返す。
fn dense_column_count(parsed: &ParsedDocument, page: &StructuredTextPage) -> Option<usize> {
    let geometry = parsed.pages.iter().find(|p| p.page_number == page.page)?;
    let width = parsed_page_display_size_points(geometry).0;
    let prose = page
        .lines
        .iter()
        .filter(|line| {
            line.text.split_whitespace().count() >= 3
                && line.text.chars().any(char::is_lowercase)
                && line.right_x - line.left_x >= structured_line_max_font_size(line) * 8.0
        })
        .cloned()
        .collect::<Vec<_>>();
    let columns = infer_text_columns(&prose, width)?;
    columns
        .iter()
        .enumerate()
        .all(|(index, _)| {
            prose
                .iter()
                .filter(|line| column_index_for_line(line, &columns) == index)
                .count()
                >= 8
        })
        .then_some(columns.len())
}
