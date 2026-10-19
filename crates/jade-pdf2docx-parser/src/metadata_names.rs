//! PDF名を借用復号し、保存容量をmetadata予算へ予約する。
use crate::decode::DecodeBudget;

/// 最外辞書のnameを一つだけ復号し、UTF-8本文の保存前に全容量を予約する。
pub(crate) fn name(budget: &DecodeBudget, dictionary: &[u8], key: &[u8]) -> Option<String> {
    value(budget, crate::metadata::value(dictionary, key)?)
}

/// 解決済みPDF name valueを容量制限付きで復号する。
pub(crate) fn value(budget: &DecodeBudget, bytes: &[u8]) -> Option<String> {
    let mut lexer = crate::syntax::Lexer { bytes, cursor: 0 };
    let crate::syntax::Token::Name(raw) = lexer.next()? else {
        return None;
    };
    if lexer.next().is_some() {
        return None;
    }
    let length = name_bytes(raw).try_fold(0usize, |length, byte| {
        byte?;
        length.checked_add(1)
    })?;
    let mut decoded = budget.metadata_vector(length)?;
    for byte in name_bytes(raw) {
        decoded.push(byte?);
    }
    String::from_utf8(decoded).ok()
}

/// escape を借用復号し、不正 escape は一つの失敗要素を返して停止する。
pub(crate) fn name_bytes(raw: &[u8]) -> impl Iterator<Item = Option<u8>> + '_ {
    let mut cursor = 0;
    std::iter::from_fn(move || {
        let byte = *raw.get(cursor)?;
        cursor += 1;
        if byte != b'#' {
            return Some(Some(byte));
        }
        let decoded = raw.get(cursor..cursor + 2).and_then(|hex| {
            let upper = (hex[0] as char).to_digit(16)?;
            let lower = (hex[1] as char).to_digit(16)?;
            Some((upper * 16 + lower) as u8)
        });
        cursor = if decoded.is_some() {
            cursor + 2
        } else {
            raw.len()
        };
        Some(decoded)
    })
}

/// 既存の byte 対応キーを、一時 byte 配列なしの fallible UTF-8 配列へ保存する。
pub(crate) fn resource_name(budget: &DecodeBudget, raw: &[u8]) -> Option<String> {
    let length = name_bytes(raw).try_fold(0usize, |length, byte| {
        length.checked_add(char::from(byte?).len_utf8())
    })?;
    let mut bytes = budget.metadata_vector::<u8>(length)?;
    for byte in name_bytes(raw) {
        let mut encoded = [0; 4];
        bytes.extend_from_slice(char::from(byte?).encode_utf8(&mut encoded).as_bytes());
    }
    String::from_utf8(bytes).ok()
}

/// 全escapeの妥当性を保存用のbyte配列を作らず検査する。
pub(crate) fn valid_resource_name(raw: &[u8]) -> bool {
    name_bytes(raw).all(|byte| byte.is_some())
}

/// 検証済みのraw名を既存byte対応UTF-8キーと、複製せず辞書順で比較する。
pub(crate) fn resource_name_order(raw: &[u8], decoded: &str) -> std::cmp::Ordering {
    name_bytes(raw)
        .map(|byte| byte.map(char::from))
        .cmp(decoded.chars().map(Some))
}

/// byte対応キーと同じ意味で二つの名前を借用復号し、不正escapeは一致させない。
pub(crate) fn same_resource_name(left: &[u8], right: &[u8]) -> bool {
    let mut left = name_bytes(left);
    let mut right = name_bytes(right);
    loop {
        match (left.next(), right.next()) {
            (Some(Some(left)), Some(Some(right))) if left == right => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

#[cfg(test)]
#[path = "metadata_name_comparison_tests.rs"]
mod comparison_tests;
