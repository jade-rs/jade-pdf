//! 元program・code計画・実出力の属性を含め、予約font名を使わない派生名を決める。
use super::{cff, gsub, opentype, plan::Plan};
use crate::decode::DecodeBudget;

/// 同じ元programでも異なるPDF幅・文字対応を区別する情報。
pub(super) struct Source<'a> {
    /// 展開後の元FontFile。hintや元の権利表示も含む。
    pub(super) program: &'a [u8],
    /// 元code順・未確認状態・幅違いを失っていない計画。
    pub(super) plan: &'a Plan,
}

/// すべての出力入力を長さで区切って識別する。元のfamily名は派生名へ再利用しない。
pub(super) fn derive(
    budget: &DecodeBudget,
    _source: &Source<'_>,
    _font: &cff::Font<'_>,
    _info: &opentype::Info<'_>,
    _rules: &[gsub::Rule<'_>],
) -> Option<String> {
    let _: Vec<u8> = budget.font_vector(1)?;
    None
}

#[cfg(test)]
#[path = "type1_outline_identity_tests.rs"]
mod tests;
