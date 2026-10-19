//! 埋め込みFontの名前・候補・結果配列をmetadata累積容量へ予約する。
use super::{
    DecodeBudget, MAX_EMBEDDED_FONT_BYTES, MAX_EMBEDDED_FONT_COUNT, ParsedEmbeddedFont,
    SfntEmbeddingPermission, ToUnicodeCMap, repair_sfnt_unicode_cmap, sfnt_embedding_permission,
    sfnt_font_signature_is_supported,
};

/// 候補は入力順の初出を選び、完成出力は旧mapと同じ名前順にする。
pub(super) fn build(
    budget: &DecodeBudget,
    source: &[u8],
    mut payload: impl FnMut(u32) -> Option<Vec<u8>>,
    mut mapping: impl FnMut(u32) -> ToUnicodeCMap,
) -> Vec<ParsedEmbeddedFont> {
    collect(budget, source, &mut payload, &mut mapping).unwrap_or_default()
}

/// どの保存・取得・修復段階で停止しても部分候補/出力を公開しない。
fn collect(
    budget: &DecodeBudget,
    source: &[u8],
    payload: &mut impl FnMut(u32) -> Option<Vec<u8>>,
    mapping: &mut impl FnMut(u32) -> ToUnicodeCMap,
) -> Option<Vec<ParsedEmbeddedFont>> {
    if budget.is_stopped() {
        return None;
    }
    let mut candidates: Vec<(String, Vec<u8>, ToUnicodeCMap)> = Vec::new();
    for descriptor in crate::metadata::objects(source, None)
        .filter(|object| crate::metadata::has_type(object.body, b"FontDescriptor"))
    {
        let name = crate::metadata_names::name(budget, descriptor.body, b"FontName");
        if budget.is_stopped() {
            return None;
        }
        let file = crate::metadata::value(descriptor.body, b"FontFile2")
            .and_then(crate::metadata::reference);
        let (Some(name), Some(file)) = (name, file) else {
            continue;
        };
        if candidates.iter().any(|entry| entry.0 == name) {
            continue;
        }
        let bytes = payload(file);
        if budget.is_stopped() {
            return None;
        }
        let Some(bytes) = bytes else {
            continue;
        };
        if bytes.is_empty()
            || bytes.len() > MAX_EMBEDDED_FONT_BYTES
            || !sfnt_font_signature_is_supported(&bytes)
        {
            continue;
        }
        if candidates.len() == candidates.capacity() {
            let capacity = candidates.capacity().saturating_mul(2).max(4);
            let mut grown = budget.metadata_vector(capacity)?;
            grown.append(&mut candidates);
            candidates = grown;
        }
        let mappings = mapping(descriptor.number);
        if budget.is_stopped() {
            return None;
        }
        candidates.push((name, bytes, mappings));
        if candidates.len() == MAX_EMBEDDED_FONT_COUNT {
            break;
        }
    }
    candidates.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let mut fonts = budget.metadata_vector(candidates.len())?;
    for (name, bytes, mapping) in candidates {
        let bytes = reusable(budget, &name, bytes, &mapping);
        if budget.is_stopped() {
            return None;
        }
        if let Some(bytes) = bytes {
            fonts.push(ParsedEmbeddedFont {
                font_name: name,
                bytes,
            });
        }
    }
    Some(fonts)
}

/// 既存の埋め込み許可・subset検査・ToUnicode修復条件を維持する。
fn reusable(
    budget: &DecodeBudget,
    name: &str,
    bytes: Vec<u8>,
    mapping: &ToUnicodeCMap,
) -> Option<Vec<u8>> {
    match sfnt_embedding_permission(&bytes) {
        SfntEmbeddingPermission::Allowed => {
            if name.as_bytes().get(6) == Some(&b'+')
                && !crate::font_validation::subset_unicode_cmap_is_complete(&bytes)
            {
                return None;
            }
            Some(bytes)
        }
        SfntEmbeddingPermission::Unspecified if !mapping.is_empty() => {
            repair_sfnt_unicode_cmap(budget, &bytes, mapping.bmp_scalar_iter())
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "embedded_fonts_tests.rs"]
mod tests;
