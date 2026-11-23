//! 元fontとPDF resourceのEncoding・Widthsを、文字codeごとに対応付ける。
use super::file::Font;
use crate::{decode::DecodeBudget, resource_resolver::ResourceResolver};

/// 一つのPDF文字codeに対応する元glyphと、PDFが宣言した幅。
#[derive(Clone, Copy)]
pub(super) struct Slot {
    /// 元fontのGID。Some(0)は.notdef、Noneは対応する元glyphが存在しない。
    pub(super) glyph: Option<u16>,
    /// 1000単位系の元PDF幅。未定義範囲はMissingWidth、未指定ならPDF既定の0。
    pub(super) width: f64,
    /// FirstCharからLastCharまでのWidthsに明示された値か。
    pub(super) explicit_width: bool,
}

/// PDFのEncoding上書きを優先し、小数幅をf32へ縮めずに原本の256codeを読む。
pub(super) fn read<'bytes>(
    _budget: &DecodeBudget,
    _font: &Font<'_>,
    _dictionary: &'bytes [u8],
    _resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Vec<Slot>> {
    None
}

#[cfg(test)]
#[path = "type1_outline_resource_tests.rs"]
mod tests;
