//! 元FontFileの宣言された区間を使い、headerと全glyphの外形をまとめて読む。
use super::{charstring::Glyph, header::Header};
use crate::decode::DecodeBudget;

/// 元の座標系を保つType1 font。UnicodeとPDF幅の適用は後段で行う。
pub(super) struct Font<'a> {
    /// 元FontFileを借用するcleartext情報。
    pub(super) header: Header<'a>,
    /// .notdefを先頭にし、残りを名前順に並べた全glyph。
    pub(super) glyphs: Vec<NamedGlyph>,
}

/// 復号bufferの寿命から独立して保つglyph。
pub(super) struct NamedGlyph {
    /// 原本のglyph名。予算へ必要byteだけを予約する。
    pub(super) name: Vec<u8>,
    /// 元programのadvanceと外形。
    pub(super) outline: Glyph,
}

/// 未変更のFontFileを二層復号し、未対応glyphや停止時は部分fontを返さない。
pub(super) fn read<'a>(
    _budget: &DecodeBudget,
    _bytes: &'a [u8],
    _length1: usize,
    _length2: usize,
) -> Option<Font<'a>> {
    None
}

#[cfg(test)]
#[path = "type1_outline_file_tests.rs"]
mod tests;
