//! PDF側の幅を優先し、フォントの表現範囲内でem単位数と外形をそろえる。
use super::charstring::Glyph;
use crate::decode::DecodeBudget;

/// 元の1000単位系で集計した、変換時の最大絶対値。
pub(super) struct Limits {
    /// 外形の座標上限。
    pub(super) coordinate: f64,
    /// Type2で格納する連続点の差分上限。
    pub(super) delta: f64,
    /// header等の符号付きmetricの上限。
    pub(super) metric: f64,
    /// PDF幅を持たないglyphも含むadvance上限。
    pub(super) advance: f64,
}

/// 実際のPDF幅の最大量子化誤差を最小化し、同等なら細かい単位を選ぶ。
pub(super) fn choose_units(budget: &DecodeBudget, widths: &[f64], limits: &Limits) -> Option<u16> {
    let _ = (budget, widths, limits);
    None
}

/// 確認済みPDF幅があればfont内advanceより優先し、外形と一緒に単位を変換する。
pub(super) fn prepare(
    budget: &DecodeBudget,
    glyph: &Glyph,
    pdf_width: Option<f64>,
    units: u16,
) -> Option<Glyph> {
    let _ = (budget, glyph, pdf_width, units);
    None
}

#[cfg(test)]
#[path = "type1_outline_scale_tests.rs"]
mod tests;
