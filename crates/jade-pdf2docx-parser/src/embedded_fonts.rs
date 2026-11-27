//! 埋め込みFontの名前・候補・結果配列をmetadata累積容量へ予約する。
use super::{
    DecodeBudget, MAX_EMBEDDED_FONT_BYTES, MAX_EMBEDDED_FONT_COUNT, ParsedEmbeddedFont,
    SfntEmbeddingPermission, ToUnicodeCMap, repair_sfnt_unicode_cmap, sfnt_embedding_permission,
    sfnt_font_signature_is_supported,
};

/// 最初に確認したdescriptorとの対応を、修復を行う最終段階まで保持する。
struct Candidate {
    /// PDFのFontName。
    name: String,
    /// 原本のFontFile2 program。
    bytes: Vec<u8>,
    /// 当該descriptorへ対応したToUnicode。
    mapping: ToUnicodeCMap,
    /// 原本のFontDescriptor object番号。
    descriptor: u32,
}

/// 候補は入力順の初出を選び、完成出力は旧mapと同じ名前順にする。
#[cfg(test)]
pub(super) fn build(
    budget: &DecodeBudget,
    source: &[u8],
    mut payload: impl FnMut(u32) -> Option<Vec<u8>>,
    mut mapping: impl FnMut(u32) -> ToUnicodeCMap,
) -> Vec<ParsedEmbeddedFont> {
    build_with_fallback(budget, source, &mut payload, &mut mapping, |_, _| None)
}

/// 検証済みの原本streamを持つ呼出し元だけに、名前欠落時の復元を任せる。
pub(super) fn build_with_fallback(
    budget: &DecodeBudget,
    source: &[u8],
    mut payload: impl FnMut(u32) -> Option<Vec<u8>>,
    mut mapping: impl FnMut(u32) -> ToUnicodeCMap,
    mut fallback: impl FnMut(&[u8], &mut dyn FnMut(u32) -> Option<Vec<u8>>) -> Option<(String, Vec<u8>)>,
) -> Vec<ParsedEmbeddedFont> {
    collect(budget, source, &mut payload, &mut mapping, &mut fallback).unwrap_or_default()
}

/// どの保存・取得・修復段階で停止しても部分候補/出力を公開しない。
fn collect(
    budget: &DecodeBudget,
    source: &[u8],
    payload: &mut impl FnMut(u32) -> Option<Vec<u8>>,
    mapping: &mut impl FnMut(u32) -> ToUnicodeCMap,
    fallback: &mut impl FnMut(
        &[u8],
        &mut dyn FnMut(u32) -> Option<Vec<u8>>,
    ) -> Option<(String, Vec<u8>)>,
) -> Option<Vec<ParsedEmbeddedFont>> {
    if budget.is_stopped() {
        return None;
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    for descriptor in crate::metadata::objects(source, None)
        .filter(|object| crate::metadata::has_type(object.body, b"FontDescriptor"))
    {
        let name = crate::metadata_names::name(budget, descriptor.body, b"FontName");
        if budget.is_stopped() {
            return None;
        }
        let file = crate::metadata::value(descriptor.body, b"FontFile2")
            .and_then(crate::metadata::reference);
        let Some(file) = file else {
            continue;
        };
        let (name, bytes) = if let Some(name) = name {
            if candidates.iter().any(|entry| entry.name == name) {
                continue;
            }
            let bytes = payload(file);
            if budget.is_stopped() {
                return None;
            }
            let Some(bytes) = bytes else {
                continue;
            };
            (name, bytes)
        } else {
            // 字句を検査してから、原本streamを持つ呼出し元へ復元を依頼する。
            let Some(raw) = crate::metadata::value(descriptor.body, b"FontName") else {
                continue;
            };
            if crate::legacy_font_names::raw_name(raw).is_none() {
                continue;
            }
            let recovered = fallback(descriptor.body, payload);
            if budget.is_stopped() {
                return None;
            }
            let Some((name, bytes)) = recovered else {
                continue;
            };
            if candidates.iter().any(|entry| entry.name == name) {
                continue;
            }
            (name, bytes)
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
        candidates.push(Candidate {
            name,
            bytes,
            mapping: mappings,
            descriptor: descriptor.number,
        });
        if candidates.len() == MAX_EMBEDDED_FONT_COUNT {
            break;
        }
    }
    candidates.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    let mut fonts = budget.metadata_vector(candidates.len())?;
    for Candidate {
        name,
        bytes,
        mapping,
        descriptor,
    } in candidates
    {
        let bytes = reusable(budget, &name, bytes, &mapping, source, descriptor);
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

/// 既存の許可・subset検査を保ち、確認済みsymbolic Macだけ明示Unicode対応を復元する。
fn reusable(
    budget: &DecodeBudget,
    name: &str,
    bytes: Vec<u8>,
    mapping: &ToUnicodeCMap,
    source: &[u8],
    descriptor: u32,
) -> Option<Vec<u8>> {
    match sfnt_embedding_permission(&bytes) {
        SfntEmbeddingPermission::Allowed => {
            if name.as_bytes().get(6) == Some(&b'+')
                && !crate::font_validation::subset_unicode_cmap_is_complete(&bytes)
            {
                return crate::font_repair::repair_symbolic_mac_unicode_cmap(
                    budget, source, descriptor, &bytes, mapping,
                );
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
