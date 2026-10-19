//! 画像の binary string 属性を、PDF の byte 表現のまま読む。
use crate::pdf_binary_string::visit_string;

/// 単一の hex/literal string を byte 列へ変換する。
pub(crate) fn string_bytes(budget: &crate::decode::DecodeBudget, value: &[u8]) -> Option<Vec<u8>> {
    if budget.is_stopped() {
        return None;
    }
    let mut length = 0usize;
    visit_string(value, |_| {
        length = length.checked_add(1)?;
        Some(())
    })?;
    let mut bytes = budget.image_vector(length)?;
    visit_string(value, |byte| {
        bytes.push(byte);
        Some(())
    })?;
    Some(bytes)
}
