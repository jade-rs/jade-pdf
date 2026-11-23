//! CFF用の元byte列と、name table用の表示全文を別々に正確な容量へ保存する。
use super::{DecodeBudget, OFL_BODY};

/// 元の表示を失わず結合し、明示OFLの場合だけ標準本文をname13へ付ける。
pub(super) fn collect(
    budget: &DecodeBudget,
    comments: &[u8],
    notice: &[u8],
    copyright: &[u8],
    ofl: bool,
) -> Option<(Vec<u8>, String)> {
    let mut length = comments.len();
    for value in [notice, copyright] {
        if !value.is_empty() {
            length = length.checked_add(value.len().checked_add(1)?)?;
        }
    }
    let mut original = budget.font_vector(length)?;
    original.extend_from_slice(comments);
    for value in [notice, copyright] {
        if budget.is_stopped() {
            return None;
        }
        if !value.is_empty() {
            original.extend_from_slice(value);
            original.push(b'\n');
        }
    }
    let mut size = original.iter().try_fold(0usize, |size, &byte| {
        if budget.is_stopped() {
            return None;
        }
        size.checked_add(char::from(byte).len_utf8())
    })?;
    if ofl {
        size = size.checked_add(1 + OFL_BODY.len())?;
    }
    let mut utf8 = budget.font_vector(size)?;
    for &byte in &original {
        if budget.is_stopped() {
            return None;
        }
        let mut encoded = [0; 4];
        utf8.extend_from_slice(char::from(byte).encode_utf8(&mut encoded).as_bytes());
    }
    if ofl {
        utf8.push(b'\n');
        utf8.extend_from_slice(OFL_BODY.as_bytes());
    }
    if budget.is_stopped() {
        return None;
    }
    Some((original, String::from_utf8(utf8).ok()?))
}
