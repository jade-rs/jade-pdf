//! 不透明な背景の確認を妨げない、既定のRGBページGroupだけを解決する。
use crate::{decode::DecodeBudget, metadata, metadata_names, resource_resolver::ResourceResolver};

/// 完全で一意なRGB・非knockoutのページGroupだけを、既存のpaint検証へ渡す。
pub(super) fn supported(budget: &DecodeBudget, source: &[u8], page: usize) -> bool {
    verified(budget, source, page).is_some()
}

/// Groupがなければ従来経路を保ち、存在すれば同じページの属性を予算付きで解決する。
fn verified(budget: &DecodeBudget, source: &[u8], page: usize) -> Option<()> {
    let pages = budget.page_dictionaries(source);
    let page = *pages.get(page)?;
    if !metadata::dictionary_entries_complete(page) {
        return None;
    }
    let mut group = None;
    for (key, value) in metadata::entries(page)? {
        if metadata_names::resource_name_order(key, "Group").is_eq() {
            if group.is_some() {
                return None;
            }
            group = Some(value);
        }
    }
    let Some(group) = group else {
        return Some(());
    };
    let objects = crate::object_index::build(budget, source)?;
    let mut resolver = ResourceResolver::new(&objects, budget);
    let (group, _) = resolver.resolve(group)?;
    if !metadata::dictionary_entries_complete(group) {
        return None;
    }
    let mut seen = 0_u8;
    for (key, raw) in metadata::entries(group)? {
        let index = ["Type", "S", "CS", "I", "K"]
            .iter()
            .position(|name| metadata_names::resource_name_order(key, name).is_eq())?;
        if seen & (1 << index) != 0 {
            return None;
        }
        seen |= 1 << index;
        let (value, _) = resolver.resolve(raw)?;
        let valid = match index {
            0 => metadata::is_name(value, b"Group"),
            1 => metadata::is_name(value, b"Transparency"),
            2 => metadata::is_name(value, b"DeviceRGB"),
            // I=trueは現段階の対象外。省略時と明示falseだけを確認する。
            3 | 4 => crate::parser::image_draws::resources::keyword(value) == Some(b"false"),
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    // /Sと明示的なDeviceRGBを必須にし、既定色空間の推測はしない。
    (seen & 0b110 == 0b110).then_some(())
}
