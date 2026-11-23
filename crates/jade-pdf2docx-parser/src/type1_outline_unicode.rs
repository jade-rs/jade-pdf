//! 確認済みPDF code/GID/幅へ、原本ToUnicodeを優先した文字列を結び付ける。
use super::{file::Font, resource::Slot};
use crate::{decode::DecodeBudget, text::ToUnicodeCMap};

/// codeごとの元の輪郭・幅と、正規化していないUnicode列。
pub(super) struct Mapping {
    /// 元resourceの対応を、未知glyphや.notdefも含めて保持する。
    pub(super) source: Slot,
    /// 未確認はNone。明示的な空文字列はSome(empty)として区別する。
    pub(super) scalars: Option<Vec<u32>>,
    /// 元ToUnicodeの明示対応を採用したか。glyph名からの補完とは区別する。
    pub(super) from_tounicode: bool,
}

/// code番号からUnicodeを推測せず、原本の文字列と全codeの対応を返す。
pub(super) fn map(
    _budget: &DecodeBudget,
    _font: &Font<'_>,
    _slots: &[Slot],
    _original: &ToUnicodeCMap,
) -> Option<Vec<Mapping>> {
    None
}

#[cfg(test)]
#[path = "type1_outline_unicode_tests.rs"]
mod tests;
