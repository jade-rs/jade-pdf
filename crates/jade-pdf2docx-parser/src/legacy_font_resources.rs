//! Rootのない互換入力から、共有索引と容量制限付き訪問集合でFontを継承する。
use super::{DecodeBudget, PageFontResourceDefinition, font_resource_definitions_from_dictionary};

/// nullを欠落として親のFontを継承し、空辞書・参照失敗では上書きを保つ。
pub(super) fn read<'a>(
    decode_budget: &DecodeBudget,
    objects: &crate::page_attributes::Objects<'a>,
    page_object: &'a str,
) -> Vec<PageFontResourceDefinition<'a>> {
    let mut object = page_object;
    let mut visited = crate::metadata_workspace::Set::default();
    let mut depth = 0usize;
    let mut resolver = crate::resource_resolver::ResourceResolver::new(objects, decode_budget);
    loop {
        if decode_budget.is_stopped() {
            return Vec::new();
        }
        if let Some(value) = crate::metadata::value(object.trim().as_bytes(), b"Resources") {
            let Some((resources, _)) = resolver.resolve(value) else {
                return Vec::new();
            };
            if !crate::page_attributes::is_null(resources) {
                return std::str::from_utf8(resources)
                    .ok()
                    .map(|resources| {
                        font_resource_definitions_from_dictionary(decode_budget, objects, resources)
                    })
                    .unwrap_or_default();
            }
        }
        let Some((parent_number, generation)) =
            crate::metadata::value(object.trim().as_bytes(), b"Parent")
                .and_then(crate::metadata::reference_with_generation)
        else {
            return Vec::new();
        };
        depth = depth.saturating_add(1);
        if decode_budget.check_page_inheritance_depth(depth).is_none() {
            return Vec::new();
        }
        if !visited
            .try_insert(decode_budget, parent_number)
            .unwrap_or(false)
        {
            return Vec::new();
        }
        let Some(&(actual, parent)) = objects.get(&parent_number) else {
            return Vec::new();
        };
        if actual != generation {
            return Vec::new();
        }
        let Ok(parent) = std::str::from_utf8(parent) else {
            return Vec::new();
        };
        object = parent;
    }
}
