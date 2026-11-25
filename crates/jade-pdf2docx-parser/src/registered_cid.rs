//! Fontの明示登録情報を検証し、固定済みAdobe-Japan1 CIDをUnicodeへ対応付ける。
use crate::{decode::DecodeBudget, metadata, resource_resolver::ResourceResolver, syntax};

/// Identity-Hと一致する登録collectionの上限だけを、Font局所に保持する。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Collection {
    /// 宣言Supplementに含まれる最後のCID。
    last: u16,
}

impl Collection {
    /// 二byte CIDを固定表から借用し、notdef・宣言範囲外は採用しない。
    pub(crate) fn lookup(self, cid: u16) -> Option<&'static str> {
        if cid == 0 || cid > self.last {
            return None;
        }
        let index = usize::from(cid) * 4;
        let offset = |i| {
            let bytes: [u8; 4] = OFFSETS.get(i..i + 4)?.try_into().ok()?;
            usize::try_from(u32::from_le_bytes(bytes)).ok()
        };
        UNICODE.get(offset(index)?..offset(index + 4)?)
    }
}

/// AdobeのBSD-3-Clause対応表を再現可能なgeneratorでUTF-8へ展開した静的本文。
const UNICODE: &str = include_str!("../resources/adobe-japan1-ucs2.utf8");
/// 各CIDのUTF-8開始位置をlittle endianで保持し、実行環境のalignmentに依存しない。
const OFFSETS: &[u8] = include_bytes!("../resources/adobe-japan1-ucs2.offsets");
/// Adobe-Japan1仕様のSupplement 0〜7が含むCID終端。
const LAST_CIDS: [u16; 8] = [8283, 8358, 8719, 9353, 15443, 20316, 23057, 23059];

/// 完全な辞書の直下に一度だけ現れる属性を借用する。
fn unique<'a>(dictionary: &'a [u8], key: &[u8], budget: &DecodeBudget) -> Option<&'a [u8]> {
    if budget.is_stopped() || !metadata::dictionary_entries_complete(dictionary) {
        return None;
    }
    let mut found = None;
    for (name, value) in metadata::entries(dictionary)? {
        if budget.is_stopped() {
            return None;
        }
        if syntax::name_is(name, key) {
            if found.is_some() {
                return None;
            }
            found = Some(value);
        }
    }
    found
}

/// 解決済みliteral/hex文字列を、固定ASCII名と確保なしで比較する。
fn string_is(value: &[u8], expected: &[u8]) -> bool {
    let mut index = 0usize;
    crate::pdf_binary_string::visit_string(value, |byte| {
        if expected.get(index) != Some(&byte) {
            return None;
        }
        index += 1;
        Some(())
    })
    .is_some()
        && index == expected.len()
}

/// ToUnicode不在・Type0・Identity-H・単一descendant・Adobe/Japan1を全て確認する。
pub(crate) fn read<'a>(
    budget: &DecodeBudget,
    dictionary: &'a [u8],
    resolver: &mut ResourceResolver<'_, 'a>,
) -> Option<Collection> {
    if budget.is_stopped() || metadata::value(dictionary, b"ToUnicode").is_some() {
        return None;
    }
    let mut value = |d, key| resolver.resolve(unique(d, key, budget)?).map(|v| v.0);
    if !metadata::is_name(value(dictionary, b"Type")?, b"Font")
        || !metadata::is_name(value(dictionary, b"Subtype")?, b"Type0")
        || !metadata::is_name(value(dictionary, b"Encoding")?, b"Identity-H")
    {
        return None;
    }
    let mut descendants = metadata::ArrayValues::new(value(dictionary, b"DescendantFonts")?, 1)?;
    let child = descendants.next_value()??;
    if descendants.next_value() != Some(None) {
        return None;
    }
    let child = resolver.resolve(child)?.0;
    let subtype = resolver.resolve(unique(child, b"Subtype", budget)?)?.0;
    if !metadata::is_name(subtype, b"CIDFontType0") && !metadata::is_name(subtype, b"CIDFontType2")
    {
        return None;
    }
    let info = resolver
        .resolve(unique(child, b"CIDSystemInfo", budget)?)?
        .0;
    let registry = resolver.resolve(unique(info, b"Registry", budget)?)?.0;
    let ordering = resolver.resolve(unique(info, b"Ordering", budget)?)?.0;
    let supplement = resolver.resolve(unique(info, b"Supplement", budget)?)?.0;
    if !string_is(registry, b"Adobe") || !string_is(ordering, b"Japan1") {
        return None;
    }
    let supplement = supplement.trim_ascii();
    let supplement = syntax::integer(supplement.strip_prefix(b"+").unwrap_or(supplement))?;
    let last = *LAST_CIDS.get(supplement)?;
    (!budget.is_stopped()).then_some(Collection { last })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits};

    /// 静的表の全CIDが空でない有効UTF-8を返し、全offsetを境界検査できる。
    #[test]
    fn complete_collection_has_valid_nonempty_unicode_sequences() {
        let collection = Collection { last: 23059 };
        for cid in 1..=23059 {
            let text = collection.lookup(cid).expect("pinned CID must exist");
            assert!(!text.is_empty() && !text.contains('\u{fffd}'));
        }
        assert!(collection.lookup(0).is_none());
        assert!(collection.lookup(23060).is_none());
    }

    /// 出力容量の不足は部分文字列を返さず、最初のResourceLimitを保持する。
    #[test]
    fn output_capacity_and_previous_failure_are_preserved() {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: 2,
            ..Default::default()
        });
        let cmap = crate::text::ToUnicodeCMap::for_registered(Collection { last: 8719 });
        assert!(
            crate::text::decode_glyph_bytes(&budget, &[0x0c, 0xd4], false, &cmap, None).is_none()
        );
        budget.invalid_registered_cid("later error must not replace capacity failure");
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                ..
            })
        ));
        assert!(budget.is_stopped());
        assert!(
            crate::text::decode_glyph_bytes(&budget, &[0x0c, 0xd4], false, &cmap, None).is_none()
        );
        assert!(budget.check().is_ok());
    }
}
