//! FontDescriptorの明示値だけを読み、名前やStemVからweightを推測しない。
use crate::{
    decode::DecodeBudget, metadata, page_attributes, resource_resolver::ResourceResolver, syntax,
};
/// 完全な辞書直下で、一度だけ現れる属性を借用する。
fn unique<'a>(dictionary: &'a [u8], key: &[u8], budget: &DecodeBudget) -> Option<&'a [u8]> {
    if !metadata::dictionary_entries_complete(dictionary) {
        return None;
    }
    let mut found = None;
    for (name, value) in metadata::entries(dictionary)? {
        budget.check().ok()?;
        if syntax::name_is(name, key) {
            if found.is_some() {
                return None;
            }
            found = Some(value);
        }
    }
    found
}
/// Type0は単一descendantを使い、simple Fontでは自分のdescriptorだけを解決する。
fn descriptor<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    dictionary: &'a [u8],
    cid: bool,
    budget: &DecodeBudget,
) -> Option<&'a [u8]> {
    let font = if cid {
        let (array, _) = resolver.resolve(unique(dictionary, b"DescendantFonts", budget)?)?;
        let mut values = metadata::ArrayValues::new(array, 1)?;
        let child = values.next_value()??;
        if values.next_value() != Some(None) {
            return None;
        }
        resolver.resolve(child)?.0
    } else {
        dictionary
    };
    resolver
        .resolve(unique(font, b"FontDescriptor", budget)?)
        .map(|(value, _)| value)
}
/// sourceの有限整数1..=1000だけを保持し、欠落・型違い・重複をNoneとする。
pub(crate) fn read<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    dictionary: &'a [u8],
    cid: bool,
    budget: &DecodeBudget,
) -> Option<u16> {
    let descriptor = descriptor(resolver, dictionary, cid, budget)?;
    let (value, _) = resolver.resolve(unique(descriptor, b"FontWeight", budget)?)?;
    let (kind, _) = resolver.resolve(unique(descriptor, b"Type", budget)?)?;
    if !metadata::is_name(kind, b"FontDescriptor") {
        return None;
    }
    let number = page_attributes::number_value(value)?;
    (number.is_finite() && number.fract() == 0. && (1. ..=1000.).contains(&number))
        .then_some(number as u16)
}
