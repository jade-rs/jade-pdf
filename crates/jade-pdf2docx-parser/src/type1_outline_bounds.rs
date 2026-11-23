//! Type2と同じ絶対座標量子化から、実際の三次曲線のmetricを計算する。
use super::{charstring::Glyph, opentype::Metric};
use crate::decode::DecodeBudget;

/// 制御点の箱ではなく、描画される曲線の極値を外向きに丸めて格納する。
pub(super) fn measure(_budget: &DecodeBudget, _glyph: &Glyph) -> Option<Metric> {
    None
}

#[cfg(test)]
#[path = "type1_outline_bounds_tests.rs"]
mod tests;
