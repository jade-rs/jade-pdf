//! FontDescriptorの明示値だけを読み、名前やStemVからweightを推測しない。
use crate::{decode::DecodeBudget, metadata, resource_resolver::ResourceResolver, syntax};
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

/// 十進表記の整数値だけを正確に読み、丸めで端数や範囲外の値を確認しない。
/// 既存互換の小数部ゼロは許し、指数や0でない小数部は採用しない。
fn exact_weight(value: &[u8], budget: &DecodeBudget) -> Option<u16> {
    let value = value.trim_ascii();
    let value = value.strip_prefix(b"+").unwrap_or(value);
    let mut number = 0u16;
    let mut fraction = false;
    let mut digits = false;
    for (index, byte) in value.iter().copied().enumerate() {
        if index % 256 == 0 {
            budget.check().ok()?;
        }
        match byte {
            b'.' if !fraction => fraction = true,
            b'0'..=b'9' => {
                digits = true;
                if fraction {
                    if byte != b'0' {
                        return None;
                    }
                } else {
                    number = number
                        .checked_mul(10)?
                        .checked_add(u16::from(byte - b'0'))?;
                    if number > 1000 {
                        return None;
                    }
                }
            }
            _ => return None,
        }
    }
    budget.check().ok()?;
    (digits && number > 0).then_some(number)
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
    exact_weight(value, budget)
}
