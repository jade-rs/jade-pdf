//! CMap 入力の走査予約と、借用を優先する fallible UTF-8 置換。
use crate::{decode::DecodeBudget, lossy_chunks::LossyChunks};
use std::borrow::Cow;

/// 本文走査を予約し、不正 UTF-8 の置換容量を検査してから文字列を作る。
pub(super) fn cmap_text<'a>(budget: &DecodeBudget, bytes: &'a [u8]) -> Option<Cow<'a, str>> {
    budget.charge_tounicode_input_bytes(bytes.len() as u64)?;
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(Cow::Borrowed(text));
    }
    let length = LossyChunks::new(bytes).fold(0usize, |n, chunk| n.saturating_add(chunk.len()));
    budget.charge_tounicode_input_bytes(length as u64)?;
    let mut text = budget.try_string_capacity(length)?;
    for chunk in LossyChunks::new(bytes) {
        text.push_str(chunk);
    }
    Some(Cow::Owned(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 有効な UTF-8 は予算予約後も元の領域を借用する。
    #[test]
    fn valid_utf8_is_borrowed() {
        let source = "日本語 CMap".as_bytes();
        let text = cmap_text(&DecodeBudget::default(), source).unwrap();
        assert!(matches!(text, Cow::Borrowed(_)));
        assert_eq!(text.as_ptr(), source.as_ptr());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// 任意の byte 列でも標準ライブラリの置換結果と一致する。
        #[test]
        fn replacement_matches_standard(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let budget = DecodeBudget::default();
            let text = cmap_text(&budget, &bytes).unwrap();
            prop_assert_eq!(text, String::from_utf8_lossy(&bytes));
            prop_assert!(budget.check().is_ok());
        }
    }
}
