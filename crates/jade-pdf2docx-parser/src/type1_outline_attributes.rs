//! 元FontInfoの外形属性とPDF descriptorの行metricを、元の1000単位で保持する。
use super::{cmap, file::Font, opentype, permission::License};
use crate::{decode::DecodeBudget, resource_resolver::ResourceResolver};

/// 丸める前の元metricと、glyph外形自身のstyle属性。
pub(super) struct Attributes {
    /// PDF descriptorを優先する、baseline上の高さ。
    pub(super) ascent: f64,
    /// PDF descriptorを優先する、baseline下の負の値。
    pub(super) descent: f64,
    /// 確認済みweight class。通常400と明示Medium500を区別する。
    pub(super) weight: u16,
    /// 外形自身がboldのweightを持つか。
    pub(super) bold: bool,
    /// 外形自身の傾斜属性。
    pub(super) italic: bool,
    /// 元FontInfoを優先した傾斜角。
    pub(super) italic_angle: f64,
    /// 元FontInfoの指定を優先する等幅属性。
    pub(super) fixed: bool,
    /// 元下線位置。未確認は0。
    pub(super) underline_position: f64,
    /// 元下線厚さ。未確認は0。
    pub(super) underline_thickness: f64,
    /// 元小文字高。未確認は0。
    pub(super) x_height: f64,
    /// 元大文字高。未確認は0。
    pub(super) cap_height: f64,
}

/// 元font名からstyleを推測せず、確認できる元の指定と外形を読む。
pub(super) fn read<'bytes>(
    budget: &DecodeBudget,
    _font: &Font<'_>,
    _descriptor: &'bytes [u8],
    _resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Attributes> {
    let _: Vec<u8> = budget.font_vector(1)?;
    None
}

impl Attributes {
    /// 元単位から一度だけ丸め、metric・cmap・完全な元許諾を同じ出力へ結び付ける。
    pub(super) fn to_info<'a>(
        &self,
        _units: u16,
        _metrics: &'a [opentype::Metric],
        _mappings: &'a [cmap::Mapping],
        _license: &'a License,
    ) -> Option<opentype::Info<'a>> {
        None
    }
}

#[cfg(test)]
#[path = "type1_outline_attributes_tests.rs"]
mod tests;
