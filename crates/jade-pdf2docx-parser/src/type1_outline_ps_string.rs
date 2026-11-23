//! Type1の未展開literal本文を、元のbyteを省略せず予算内で復号する。
use crate::decode::DecodeBudget;

/// 計測・検証後に必要なbyteだけを累積予約し、失敗では部分文字列を公開しない。
pub(super) fn decode(budget: &DecodeBudget, raw: &[u8]) -> Option<Vec<u8>> {
    if raw.len() > 65535 || budget.is_stopped() {
        return None;
    }
    let mut length = 0usize;
    crate::pdf_binary_string::visit_literal_body(raw, |_| {
        length = length.checked_add(1)?;
        Some(())
    })?;
    let mut output = budget.font_vector(length)?;
    crate::pdf_binary_string::visit_literal_body(raw, |byte| {
        output.push(byte);
        Some(())
    })?;
    (!budget.is_stopped()).then_some(output)
}

#[cfg(test)]
#[path = "type1_outline_ps_string_tests.rs"]
mod tests;
