//! 復号済みPrivate辞書の静的なglyph/subroutine格納だけを読む。
use crate::decode::DecodeBudget;

/// 同じ復号bufferを借用する一glyphの暗号化program。
pub(super) struct Entry<'a> {
    /// 原本のglyph名。
    pub(super) name: &'a [u8],
    /// RDで長さを宣言したcharstring byte列。
    pub(super) bytes: &'a [u8],
}

/// 動的なPostScriptを実行せずに確認できた辞書。
pub(super) struct Private<'a> {
    /// charstringで捨てるprefix。NoneはlenIV=-1の平文。
    pub(super) prefix: Option<usize>,
    /// 未定義slotを区別したsubroutine。
    pub(super) subrs: Vec<Option<&'a [u8]>>,
    /// 元の名前とprogramを保つglyph。
    pub(super) glyphs: Vec<Entry<'a>>,
}

/// 既知の静的なRD/ND/NP定義と、長さで区切った辞書だけを読む。
pub(super) fn parse<'a>(budget: &DecodeBudget, bytes: &'a [u8]) -> Option<Private<'a>> {
    let _ = (budget, bytes);
    None
}

#[cfg(test)]
#[path = "type1_outline_private_tests.rs"]
mod tests;
