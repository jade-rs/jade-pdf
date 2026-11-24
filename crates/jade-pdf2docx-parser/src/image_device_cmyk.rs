//! DeviceCMYK JPEGを四成分で復号し、source Decode・Matteの後に既存のRGB近似へ渡す。
use super::{
    DecodeBudget, image_attribute,
    image_sample_decode::{SampleDecode, number},
};

/// 検証済み四成分と透明度を使い、RGB自動変換によるPDF色極性の反転を防ぐ。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if number(
        budget,
        pdf,
        &image_attribute(budget, pdf, dict, b"BitsPerComponent")?,
    )? != 8.0
    {
        return None;
    }
    let samples = super::image_icc::cmyk_jpeg_samples(budget, pdf, dict, data, width, height)?;
    let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if count == 0 || samples.len() != count.checked_mul(4)? {
        return None;
    }
    let mapping = SampleDecode::read_cmyk(budget, pdf, dict)?;
    let mask = if super::image_smask::is_present(dict) {
        let text = crate::metadata_input::text(budget, dict)?;
        Some(super::image_smask::decode_with_verified_source(
            budget, pdf, &text, width, height,
        )?)
    } else {
        None
    };
    let mut rgb = budget.image_vector(count.checked_mul(3)?)?;
    for (index, pixel) in samples.chunks_exact(4).enumerate() {
        if budget.is_stopped() {
            return None;
        }
        let mut components = [0f32; 4];
        for (channel, value) in components.iter_mut().enumerate() {
            let decoded = mapping.rgb_component(pixel[channel], channel) / 255.0;
            *value = mask.as_ref().map_or(decoded, |mask| {
                mask.source_component(decoded, index, channel)
            }) as f32;
        }
        let [c, m, y, k] = components;
        rgb.extend_from_slice(&super::pdf_cmyk_to_rgb(c, m, y, k));
    }
    if let Some(mask) = mask {
        let rgba = super::image_pixels::with_alpha(budget, &rgb, &mask.alpha)?;
        super::encode_rgba8_png(budget, width, height, &rgba)
    } else {
        super::encode_rgb8_png(budget, width, height, &rgb)
    }
}
