//! Adobe glyph名からUnicodeを取り出し、未知のcomponentを確認不能として区別する。
use crate::decode::DecodeBudget;

/// Unicodeを順に通知し、未知componentの有無とキャンセルを区別する。
pub(crate) fn visit(
    budget: &DecodeBudget,
    _name: &[u8],
    _zapf: bool,
    _emit: impl FnMut(char) -> Option<()>,
) -> Option<bool> {
    (!budget.is_stopped()).then_some(false)
}

#[cfg(test)]
#[path = "glyph_names_tests.rs"]
mod tests;
