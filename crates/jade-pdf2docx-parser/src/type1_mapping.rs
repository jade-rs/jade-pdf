//! 単byteのType1 glyph対応を既存CMapへ合成し、未知slotを別に保存する。
use super::{ToUnicodeCMap, ToUnicodeCMapData};
use crate::{
    decode::DecodeBudget,
    type1_program::{Base, Encoding},
};
use std::borrow::Cow;

use crate::type1_standard_encoding as standard;

/// 完成Unicode表と、ToUnicodeでも確認できない256個のcode。
#[derive(Clone)]
pub(crate) struct Mapping {
    /// 元ToUnicodeを優先した共有表。
    pub(crate) cmap: ToUnicodeCMap,
    /// 描画時にだけ品質欠落とするbit列。
    pub(crate) unknown: [u64; 4],
}

impl Mapping {
    /// 追加のglyph対応がない場合は元CMapを共有し、使用codeの確認不能状態だけを作る。
    pub(crate) fn unverified(budget: &DecodeBudget, original: &ToUnicodeCMap) -> Option<Self> {
        let mut unknown = [u64::MAX; 4];
        let digits = b"0123456789ABCDEF";
        for code in 0..256 {
            if budget.is_stopped() {
                return None;
            }
            let key_bytes = [digits[code >> 4], digits[code & 15]];
            let key = std::str::from_utf8(&key_bytes).ok()?;
            if original.key_is_allowed(key) && original.mappings.get(key).is_some() {
                unknown[code / 64] &= !(1u64 << (code % 64));
            }
        }
        Some(Self {
            cmap: original.clone(),
            unknown,
        })
    }
}

/// 作者のglyph名をUnicodeへ変換し、元codeからのASCII推測を防ぐ。
pub(crate) fn build(
    budget: &DecodeBudget,
    encoding: Option<&Encoding<'_>>,
    differences: &[Option<&[u8]>; 256],
    original: &ToUnicodeCMap,
) -> Option<Mapping> {
    let mut data = ToUnicodeCMapData::default();
    let mut unknown = [0; 4];
    for code in 0..256 {
        budget.charge_tounicode_entries(1)?;
        let digits = b"0123456789ABCDEF";
        let key_bytes = [digits[code >> 4], digits[code & 15]];
        let key = std::str::from_utf8(&key_bytes).ok()?;
        let original_value = original
            .key_is_allowed(key)
            .then(|| original.mappings.get(key))
            .flatten();
        let difference = match differences[code] {
            Some(raw) => Some(difference_name(budget, raw)?),
            None => None,
        };
        let name = difference.as_deref().or_else(|| {
            encoding.and_then(|table| {
                table.slots[code].or_else(|| match table.base {
                    Base::Standard => Some(standard::STANDARD[code]),
                    // Latin1の表を未検証のbyte→Unicode推測で代用しない。
                    Base::Custom | Base::Latin1 => None,
                })
            })
        });
        let mut length = 0usize;
        let complete = if original_value.is_some() {
            true
        } else if let Some(name) = name {
            crate::glyph_names::visit(budget, name, false, |scalar| {
                length = length.checked_add(scalar.len_utf8())?;
                Some(())
            })?
        } else {
            false
        };
        let payload = original_value.map_or_else(
            || {
                if length == 0 {
                    '\u{fffd}'.len_utf8()
                } else {
                    length
                }
            },
            String::len,
        );
        budget.charge_tounicode_bytes((2 + payload) as u64)?;
        data.reserve_mapping(budget)?;
        let mut stored_key = budget.try_string_capacity(2)?;
        stored_key.push_str(key);
        let mut value = budget.try_string_capacity(payload)?;
        if let Some(original_value) = original_value {
            value.push_str(original_value);
        } else {
            if let Some(name) = name {
                crate::glyph_names::visit(budget, name, false, |scalar| {
                    value.push(scalar);
                    Some(())
                })?;
            }
            if value.is_empty() {
                value.push('\u{fffd}');
            }
        }
        if !complete {
            unknown[code / 64] |= 1u64 << (code % 64);
        }
        data.insert_mapping(stored_key, value);
    }
    data.insert_code_space_range(budget, "00", "FF")?;
    Some(Mapping {
        cmap: data.finish(budget),
        unknown,
    })
}

/// PDF側の名前だけを予算内でescape復号し、不正名は未知slotとして残す。
fn difference_name<'a>(budget: &DecodeBudget, raw: &'a [u8]) -> Option<Cow<'a, [u8]>> {
    budget.charge_tounicode_input_bytes(raw.len() as u64)?;
    for byte in crate::metadata_names::name_bytes(raw) {
        if budget.is_stopped() {
            return None;
        }
        if byte.is_none() {
            return Some(Cow::Borrowed(b""));
        }
    }
    if !raw.contains(&b'#') {
        return Some(Cow::Borrowed(raw));
    }
    let mut bytes = budget.metadata_vector(raw.len())?;
    for byte in crate::metadata_names::name_bytes(raw) {
        if budget.is_stopped() {
            return None;
        }
        bytes.push(byte?);
    }
    Some(Cow::Owned(bytes))
}
