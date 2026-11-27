//! stroke用ExtGStateを所属ページで解決し、未対応の合成・mask・効果を引き継がない。
use crate::{decode::DecodeBudget, metadata, resource_resolver::ResourceResolver};

/// 完全かつ一意なgs辞書の、stroke alpha差分だけを返す。
pub(in crate::parser) fn resolve<'a>(
    budget: &DecodeBudget,
    resolver: &mut ResourceResolver<'_, 'a>,
    resources: Option<&'a [u8]>,
    name: &[u8],
) -> Option<Option<f64>> {
    let (states, _) = resolver.resolve(metadata::value(resources?, b"ExtGState")?)?;
    if !metadata::dictionary_entries_complete(states) {
        return None;
    }
    let mut selected = None;
    for (key, value) in metadata::entries(states)? {
        if budget.is_stopped() {
            return None;
        }
        if crate::metadata_names::same_resource_name(key, name) && selected.replace(value).is_some()
        {
            return None;
        }
    }
    let (state, _) = resolver.resolve(selected?)?;
    if !metadata::dictionary_entries_complete(state) {
        return None;
    }
    let keys = [
        "Type", "CA", "ca", "BM", "SMask", "AIS", "OP", "op", "OPM", "SA", "TK",
    ];
    let mut seen = 0_u16;
    let mut opacity = None;
    for (key, value) in metadata::entries(state)? {
        if budget.is_stopped() {
            return None;
        }
        let index = keys
            .iter()
            .position(|name| crate::metadata_names::resource_name_order(key, name).is_eq())?;
        if seen & (1 << index) != 0 {
            return None;
        }
        seen |= 1 << index;
        let (value, _) = resolver.resolve(value)?;
        let valid = match index {
            0 => metadata::is_name(value, b"ExtGState"),
            1 => {
                opacity = Some(crate::parser::image_draws::resources::alpha(
                    crate::parser::image_draws::resources::keyword(value)?,
                )?);
                true
            }
            2 => crate::parser::image_draws::resources::alpha(
                crate::parser::image_draws::resources::keyword(value)?,
            )
            .is_some(),
            3 => metadata::is_name(value, b"Normal"),
            4 => metadata::is_name(value, b"None"),
            5 | 6 | 7 | 9 | 10 => matches!(
                crate::parser::image_draws::resources::keyword(value)?,
                b"true" | b"false"
            ),
            8 => matches!(
                crate::parser::image_draws::resources::keyword(value)?,
                b"0" | b"1"
            ),
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    Some(opacity)
}

/// Device色が既定置換されていないことを確認し、未知の色変換から黒を推測しない。
pub(in crate::parser) fn native_device<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    resources: Option<&'a [u8]>,
    components: Option<usize>,
) -> bool {
    let name = match components {
        Some(1) => "DefaultGray",
        Some(3) => "DefaultRGB",
        Some(4) => "DefaultCMYK",
        _ => return false,
    };
    let Some(raw) = resources.and_then(|r| metadata::value(r, b"ColorSpace")) else {
        return true;
    };
    let Some((spaces, _)) = resolver.resolve(raw) else {
        return false;
    };
    if !metadata::dictionary_entries_complete(spaces) {
        return false;
    }
    metadata::entries(spaces).is_some_and(|mut entries| {
        !entries.any(|(key, _)| crate::metadata_names::resource_name_order(key, name).is_eq())
    })
}

/// 互換入力でも最寄りの非null Resourcesを親から解決し、循環と世代違いを拒否する。
pub(in crate::parser) fn inherited<'a>(
    budget: &DecodeBudget,
    resolver: &mut ResourceResolver<'_, 'a>,
    mut node: &'a [u8],
) -> Option<Option<&'a [u8]>> {
    let mut visited = crate::metadata_workspace::Set::default();
    loop {
        if budget.is_stopped() {
            return None;
        }
        if let Some(raw) = metadata::value(node, b"Resources") {
            let (resources, _) = resolver.resolve(raw)?;
            if !crate::page_attributes::is_null(resources) {
                return metadata::dictionary_entries_complete(resources).then_some(Some(resources));
            }
        }
        let Some(parent) = metadata::value(node, b"Parent") else {
            return Some(None);
        };
        if crate::page_attributes::is_null(parent) {
            return Some(None);
        }
        let reference = metadata::reference_with_generation(parent)?;
        if !visited.try_insert(budget, reference)? {
            return None;
        }
        let (next, _) = resolver.resolve(parent)?;
        if !metadata::dictionary_entries_complete(next) {
            return None;
        }
        node = next;
    }
}
