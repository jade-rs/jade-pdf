//! ToUnicode参照の字句を借用し、結果配列の全要求容量を確保前に予約する。
use crate::{decode::DecodeBudget, metadata_workspace::try_push};

/// object number順・重複除去を維持し、停止時は先行参照も破棄する。
pub(super) fn parse(budget: &DecodeBudget, bytes: &[u8]) -> Vec<u32> {
    if budget.is_stopped() {
        return Vec::new();
    }
    let mut refs = Vec::new();
    let mut cursor = 0usize;
    while let Some(found) = super::find_bytes(&bytes[cursor..], b"/ToUnicode") {
        if budget.is_stopped() {
            return Vec::new();
        }
        cursor += found + b"/ToUnicode".len();
        let mut tokens = bytes[cursor..]
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|token| !token.is_empty())
            .map(trim_delimiters);
        let (Some(number), Some(generation), Some(marker)) =
            (tokens.next(), tokens.next(), tokens.next())
        else {
            continue;
        };
        if marker == b"R"
            && !generation.is_empty()
            && generation.iter().all(u8::is_ascii_digit)
            && let Some(number) = decimal(number)
            && try_push(budget, &mut refs, number).is_none()
        {
            return Vec::new();
        }
    }
    refs.sort_unstable();
    refs.dedup();
    refs
}

/// 前後のdictionary/list delimiterだけを借用sliceから除去する。
fn trim_delimiters(token: &[u8]) -> &[u8] {
    let delimiter = |byte: &u8| matches!(byte, b'<' | b'>' | b'[' | b']' | b'(' | b')' | b'/');
    let start = token
        .iter()
        .position(|byte| !delimiter(byte))
        .unwrap_or(token.len());
    let end = token
        .iter()
        .rposition(|byte| !delimiter(byte))
        .map_or(start, |index| index + 1);
    &token[start..end]
}

/// 非数字・空値・u32超過を計算前に拒否する十進object number。
fn decimal(token: &[u8]) -> Option<u32> {
    if token.is_empty() {
        return None;
    }
    token.iter().try_fold(0u32, |value, byte| {
        if !byte.is_ascii_digit() {
            return None;
        }
        value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
    })
}
