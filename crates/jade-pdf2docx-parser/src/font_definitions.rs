//! Font定義slotとresource名を予約し、inline辞書は元入力から借用する。
use crate::{
    decode::DecodeBudget, metadata, page_attributes::Objects, resource_resolver::ResourceResolver,
};

/// page Resources内のFont resource定義。
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Definition<'a> {
    /// content streamのTfが参照するresource名。
    pub(super) resource_name: String,
    /// indirect Font objectの場合のobject number。
    pub(super) object_number: Option<u32>,
    /// inline Font dictionaryの場合のコピーしない辞書本文。
    pub(super) inline_dictionary: Option<&'a str>,
}

/// 全候補slotを確保前に予約し、停止時は部分定義を破棄する。
pub(super) fn extract<'a>(
    budget: &DecodeBudget,
    objects: &Objects<'a>,
    resources: &'a str,
) -> Vec<Definition<'a>> {
    let Some(value) = metadata::value(resources.as_bytes(), b"Font") else {
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
        let Some((dictionary, object_number)) = resolver.resolve(value) else {
            if budget.is_stopped() {
                return Vec::new();
            }
            continue;
        };
        if !matches!(
            (crate::syntax::Lexer {
                bytes: dictionary,
                cursor: 0
            })
            .next(),
            Some(crate::syntax::Token::Open(true))
        ) {
            continue;
        }
        let inline_dictionary = if object_number.is_none() {
            let Ok(dictionary) = std::str::from_utf8(dictionary) else {
                continue;
            };
            Some(dictionary)
        } else {
            None
        };
        if let Some(resource_name) = crate::metadata_names::resource_name(budget, name) {
            result.push(Definition {
                resource_name,
                object_number,
                inline_dictionary,
            });
        }
        if budget.is_stopped() {
            return Vec::new();
        }
    }
    result
}
