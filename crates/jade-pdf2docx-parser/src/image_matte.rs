//! 正確なsource色の逆変換が可能なMatte属性だけを受け付ける。
use super::{DecodeBudget, ImageFilter, image_attribute};

/// optional Matteを解決し、成分数・範囲を検証する。
pub(super) fn read(
    budget: &DecodeBudget,
    pdf: &[u8],
    parent: &[u8],
    mask: &[u8],
    source_components_verified: bool,
) -> Option<Option<[f64; 4]>> {
    if crate::metadata::value(mask, b"Matte").is_none() {
        return Some(None);
    }
    let value = image_attribute(budget, pdf, mask, b"Matte")?;
    if super::single_word(&value) == Some(b"null") {
        return Some(None);
    }
    // codec画像はsource成分の検証が済んだ専用入口だけを許可する。
    if !source_components_verified && ImageFilter::read(parent)? != ImageFilter::Flate {
        return None;
    }
    let color = image_attribute(budget, pdf, parent, b"ColorSpace")?;
    let components = if crate::metadata::is_name(&color, b"DeviceRGB") {
        3
    } else if crate::metadata::is_name(&color, b"DeviceGray")
        || (source_components_verified
            && super::super::image_separation::is_separation(budget, pdf, parent)?)
    {
        1
    } else if source_components_verified && super::super::image_icc::is_icc(budget, pdf, parent)? {
        super::super::image_icc::components(budget, pdf, parent)?
    } else {
        return None;
    };
    let mut entries = crate::metadata::array_entries(&value, components)?;
    let mut matte = [0.0; 4];
    for component in matte.iter_mut().take(components) {
        let value = super::super::image_sample_decode::number(budget, pdf, entries.next()??)?;
        if !(0.0..=1.0).contains(&value) {
            return None;
        }
        *component = value;
    }
    if entries.next().is_some() {
        return None;
    }
    if components == 1 {
        matte = [matte[0]; 4];
    }
    Some(Some(matte))
}
