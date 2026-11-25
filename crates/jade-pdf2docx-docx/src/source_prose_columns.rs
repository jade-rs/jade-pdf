//! 表の所有やfooter抽出に先立ち、原本の長い散文列を照合する。
use super::*;

/// 左右各八行以上の長い散文がある原本では、表や未検証footerへ昇格しない。
pub(super) fn has_dense_rows(parsed: &ParsedDocument, page: &StructuredTextPage) -> bool {
    dense_column_count(parsed, page).is_some()
}

/// Wordで配置を確認した二列だけを、旧表推定から実段組へ戻す対象にする。
pub(super) fn has_two_dense_columns(parsed: &ParsedDocument, page: &StructuredTextPage) -> bool {
    dense_column_count(parsed, page) == Some(2)
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
