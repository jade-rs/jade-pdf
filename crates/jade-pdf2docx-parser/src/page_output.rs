//! 公開IRのページ保存容量を、表示矩形とは別に予約する。
use super::{DecodeBudget, ParsedPage, ParsedPageViewport};

/// 公開ページ配列の全要求容量を予約し、表示矩形を維持して保存する。
pub(super) fn publish_pages(
    budget: &DecodeBudget,
    viewports: &[ParsedPageViewport],
) -> Vec<ParsedPage> {
    let Some(mut pages) = budget.metadata_vector(viewports.len()) else {
        return Vec::new();
    };
    pages.extend(viewports.iter().map(|viewport| viewport.page.clone()));
    pages
}
