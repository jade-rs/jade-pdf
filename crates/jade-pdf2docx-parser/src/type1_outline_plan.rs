//! 原本glyphを保ちながら、PDF codeごとに異なる幅とUnicode列を出力計画へ残す。
use super::{file::Font, unicode::Mapping};
use crate::decode::DecodeBudget;

/// 元glyphとその幅違いを含む、出力GID順の計画。
pub(super) struct Plan {
    /// 全原本glyphを先頭に保ち、必要な幅違いだけを後ろへ追加する。
    pub(super) glyphs: Vec<Glyph>,
    /// 元の256code順に保持する、Unicodeと外形の対応。
    pub(super) codes: Vec<Code>,
}

/// 元の外形を複製せず参照する、一つの出力glyph。
pub(super) struct Glyph {
    /// 原本FontFile内のGID。外形とglyph名は原本に由来する。
    pub(super) original: u16,
    /// PDFが上書きする幅。Noneは元programのadvanceを保つ。
    pub(super) pdf_width: Option<f64>,
}

/// Unicode文字列と描画glyphを同一視しない、一つのPDF code。
pub(super) struct Code {
    /// 空文字列・未知文字・明示的ToUnicodeと元の幅をそのまま保つ。
    pub(super) mapping: Mapping,
    /// 確認済みの非.notdef出力GID。欠落と.notdefの区別はmapping内に残す。
    pub(super) glyph: Option<u16>,
}

/// codeをUnicodeやglyph名から推測せず、幅違いと複数scalarを計画する。
pub(super) fn build(
    _budget: &DecodeBudget,
    _font: &Font<'_>,
    _mappings: Vec<Mapping>,
) -> Option<Plan> {
    None
}

#[cfg(test)]
#[path = "type1_outline_plan_tests.rs"]
mod tests;
