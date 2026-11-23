//! Contentsの描画順・繰り返しを保持し、参照保存を累積予算で制限する。
use crate::{decode::DecodeBudget, metadata};

/// ページ辞書直下の参照だけを、コメント・文字列・入れ子を除いて借用走査する。
pub(crate) fn from_page(object: &[u8]) -> impl Iterator<Item = u32> {
    let value = metadata::dictionary_iter(object)
        .next()
        .and_then(|dictionary| metadata::value(dictionary, b"Contents"))
        .unwrap_or(b"");
    metadata::reference(value).into_iter().chain(
        metadata::array_entries(value, usize::MAX)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(metadata::reference),
    )
}

/// 検証済み列を優先し、Rootなし互換入力でも同じstreamの実行を省略しない。
pub(crate) fn page_references(budget: &DecodeBudget, bytes: &[u8]) -> Vec<(usize, u32)> {
    if let Some(references) = budget.page_content_references() {
        return references;
    }
    let pages = budget.page_dictionaries(bytes);
    let count = pages.iter().map(|page| from_page(page).count()).sum();
    let Some(mut result) = budget.metadata_vector(count) else {
        return Vec::new();
    };
    for (page, dictionary) in pages.into_iter().enumerate() {
        for object in from_page(dictionary) {
            result.push((page, object));
        }
    }
    result
}

#[cfg(test)]
#[path = "content_refs_tests.rs"]
mod tests;
