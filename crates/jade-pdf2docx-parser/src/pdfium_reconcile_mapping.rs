//! sourceの明示空白・検証済みvariation sequence・SDK語間を容量内で共存させる。
use crate::decode::DecodeBudget;

/// 一つのsource spanと対応するnative文字・語間位置。
pub(super) struct Mapping<'a> {
    /// 同じページの未消費native scalar。
    pub(super) native: &'a [char],
    /// nativeが省いたsource selectorを、そのbase文字と一緒に保持するか。
    pub(super) preserve_variations: bool,
    /// 完全にUnicodeが一致したページでSDKが識別した語間位置。
    pub(super) spaces: &'a [usize],
    /// source spanの先頭に対応するnativeの非空白位置。
    pub(super) offset: usize,
}

impl Mapping<'_> {
    /// 二回の走査で全UTF-8容量を数え、部分文字列を公開せずに復元する。
    pub(super) fn replacement(&self, budget: &DecodeBudget, pure: &str) -> Option<String> {
        let mut capacity = 0usize;
        self.visit(budget, pure, |scalar| {
            capacity = capacity.checked_add(scalar.len_utf8())?;
            Some(())
        })?;
        let mut output = budget.metadata_vector(capacity)?;
        self.visit(budget, pure, |scalar| {
            let mut utf8 = [0; 4];
            output.extend_from_slice(scalar.encode_utf8(&mut utf8).as_bytes());
            Some(())
        })?;
        String::from_utf8(output).ok()
    }

    /// 明示空白を保持し、span内の確認済み語間だけを一つ追加する。
    fn visit(
        &self,
        budget: &DecodeBudget,
        pure: &str,
        mut emit: impl FnMut(char) -> Option<()>,
    ) -> Option<()> {
        let mut consumed = 0usize;
        let mut last_was_space = true;
        for scalar in pure.chars() {
            if budget.is_stopped() {
                return None;
            }
            if scalar.is_whitespace() {
                emit(scalar)?;
                last_was_space = true;
            } else if self.preserve_variations && super::variation_selector(scalar) {
                emit(scalar)?;
            } else {
                if consumed > 0
                    && !last_was_space
                    && self
                        .spaces
                        .binary_search(&self.offset.checked_add(consumed)?)
                        .is_ok()
                {
                    emit(' ')?;
                }
                emit(*self.native.get(consumed)?)?;
                consumed = consumed.checked_add(1)?;
                last_was_space = false;
            }
        }
        Some(())
    }
}
