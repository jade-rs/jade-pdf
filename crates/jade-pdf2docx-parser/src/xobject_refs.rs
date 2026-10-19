//! XObject 辞書を借用し、名前・ページ別保存・最終結果の累積確保を制限する。
use crate::metadata_names::resource_name;
use crate::resource_resolver::ResourceResolver;
use crate::{decode::DecodeBudget, metadata, page_attributes::Objects};

/// 最外の XObject 辞書から参照を読み、結果配列と名前を確保前に予約する。
pub(crate) fn from_resources(
    budget: &DecodeBudget,
    objects: &Objects<'_>,
    resources: &[u8],
) -> Vec<(String, u32)> {
    let Some(value) = metadata::value(resources, b"XObject") else {
        return Vec::new();
    };
    let mut resolver = ResourceResolver::new(objects, budget);
    let Some((dictionary, _)) = resolver.resolve(value) else {
        return Vec::new();
    };
    let entries = || metadata::entries(dictionary).into_iter().flatten();
    let Some(mut result) = budget.metadata_vector(entries().count()) else {
        return Vec::new();
    };
    for (name, value) in entries() {
        let Some((_, Some(number))) = resolver.resolve(value) else {
            continue;
        };
        if let Some(name) = resource_name(budget, name) {
            result.push((name, number));
        }
        if budget.is_stopped() {
            return Vec::new();
        }
    }
    if budget.is_stopped() {
        Vec::new()
    } else {
        result
    }
}

/// Root なしのページでも Resources と XObject の最外キーだけを読む。
fn from_page(budget: &DecodeBudget, objects: &Objects<'_>, page: &[u8]) -> Vec<(String, u32)> {
    let Some(dictionary) = metadata::dictionary_iter(page).next() else {
        return Vec::new();
    };
    let resources = metadata::value(dictionary, b"Resources").unwrap_or(dictionary);
    let mut resolver = ResourceResolver::new(objects, budget);
    let Some((resources, _)) = resolver.resolve(resources) else {
        return Vec::new();
    };
    from_resources(budget, objects, resources)
}

/// 全ページの中間配列と最終配列を予約し、名前を複製せず描画参照を移す。
pub(crate) fn page_references(budget: &DecodeBudget, source: &[u8]) -> Vec<(usize, String, u32)> {
    let resources = budget.page_resources(source);
    let Some(objects) = crate::object_index::build(budget, source) else {
        return Vec::new();
    };
    let pages = budget.page_dictionaries(source);
    let Some(mut selected) = budget.metadata_vector::<Vec<(String, u32)>>(pages.len()) else {
        return Vec::new();
    };
    for (page, dictionary) in pages.into_iter().enumerate() {
        selected.push(match &resources {
            Some(resources) => resources[page]
                .map(|dictionary| from_resources(budget, &objects, dictionary))
                .unwrap_or_default(),
            None => from_page(budget, &objects, dictionary),
        });
        if budget.is_stopped() {
            return Vec::new();
        }
    }
    let Some(mut result) = budget.metadata_vector(selected.iter().map(Vec::len).sum()) else {
        return Vec::new();
    };
    for (page, references) in selected.into_iter().enumerate() {
        result.extend(
            references
                .into_iter()
                .map(|(name, number)| (page, name, number)),
        );
    }
    result
}

#[cfg(test)]
#[path = "xobject_refs_tests.rs"]
mod tests;
