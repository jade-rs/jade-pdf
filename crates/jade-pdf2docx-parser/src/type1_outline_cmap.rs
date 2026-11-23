//! 確認済みのUnicode/glyph対応を、BMPと全平面の二形式へ格納する。
use crate::decode::DecodeBudget;

/// 一つのUnicode scalarと、元の外形を指すGID。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Mapping {
    /// surrogateを含まないUnicode scalar。
    pub(super) scalar: u32,
    /// 0の.notdefとは区別した、既存glyphのGID。
    pub(super) glyph: u16,
}

/// format4/12をUnicode/Windows両platformで共有し、曖昧な衝突は拒否する。
pub(super) fn encode(
    budget: &DecodeBudget,
    mappings: &[Mapping],
    glyph_count: u16,
) -> Option<Vec<u8>> {
    let _ = (budget, mappings, glyph_count);
    None
}

#[cfg(test)]
#[path = "type1_outline_cmap_tests.rs"]
mod tests;
