//! Contents の値を借用走査し、ページ順の参照保存と重複除去を累積予算で制限する。
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

/// 同一ページ内の重複を除いても、最初の描画順を復元できる参照。
struct OrderedReference {
    /// 所属するページの入力順番号。
    page: usize,
    /// Contents stream の object 番号。
    object: u32,
    /// 重複除去前の全ページ通算の入力順。
    order: usize,
}

/// 作業配列と結果配列を確保前に予約し、ページ順・初出順を保持して返す。
pub(crate) fn page_references(budget: &DecodeBudget, bytes: &[u8]) -> Vec<(usize, u32)> {
    let pages = budget.page_dictionaries(bytes);
    let count = pages.iter().map(|page| from_page(page).count()).sum();
    let Some(mut ordered) = budget.metadata_vector::<OrderedReference>(count) else {
        return Vec::new();
    };
    for (page, dictionary) in pages.into_iter().enumerate() {
        for object in from_page(dictionary) {
            ordered.push(OrderedReference {
                page,
                object,
                order: ordered.len(),
            });
        }
    }
    // unstable sort は追加配列を確保せず、初出の参照を先頭へ置く。
    ordered.sort_unstable_by_key(|reference| (reference.page, reference.object, reference.order));
    ordered.dedup_by_key(|reference| (reference.page, reference.object));
    ordered.sort_unstable_by_key(|reference| reference.order);
    let Some(mut result) = budget.metadata_vector(ordered.len()) else {
        return Vec::new();
    };
    result.extend(
        ordered
            .into_iter()
            .map(|reference| (reference.page, reference.object)),
    );
    result
}

#[cfg(test)]
#[path = "content_refs_tests.rs"]
mod tests;
