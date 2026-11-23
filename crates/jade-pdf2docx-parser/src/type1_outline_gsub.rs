//! 原文の複数scalarを互換文字へ置換せず、確認済みの合字GIDへ結ぶGSUBを作る。
use crate::decode::DecodeBudget;

/// cmapで得るcomponent GID列と、原本の外形を持つ出力GID。
pub(super) struct Rule<'a> {
    /// 論理順のcomponent。先頭を含め二つ以上で、.notdefを使わない。
    pub(super) components: &'a [u16],
    /// 原本の合字外形に対応するGID。
    pub(super) glyph: u16,
}

/// 長い列を優先し、16bit境界を越える内容はtype7の32bit offsetで参照する。
pub(super) fn encode(
    _budget: &DecodeBudget,
    _rules: &[Rule<'_>],
    _glyph_count: u16,
) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
#[path = "type1_outline_gsub_tests.rs"]
mod tests;
