//! 検証済みCMYK ICC画像を四成分のまま補正してsRGBへ変換する。
use super::super::{DecodeBudget, ImageFilter, image_attribute, image_sample_decode::SampleDecode};
use moxcms::{BarycentricWeightScale, ColorProfile, Layout, TransformOptions};

/// source Decode・Matte・ICC変換の途中で量子化せず、最終RGBにalphaを付ける。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let filter = ImageFilter::read(dict)?;
    if !matches!(filter, ImageFilter::Flate | ImageFilter::Dct) {
        return None;
    }
    let color = image_attribute(budget, pdf, dict, b"ColorSpace")?;
    let profile = super::cmyk::read(budget, pdf, &color)?;
    let transform = profile
        .create_transform_f64(
            Layout::Rgba,
            &ColorProfile::new_srgb(),
            Layout::Rgb,
            TransformOptions {
                barycentric_weight_scale: BarycentricWeightScale::High,
                rendering_intent: super::intent(budget, pdf, dict)?,
                allow_use_cicp_transfer: false,
                prefer_fixed_point: false,
                ..Default::default()
            },
        )
        .ok()?;
    let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if count == 0 {
        return None;
    }
    let length = count.checked_mul(4)?;
    let samples = if filter == ImageFilter::Dct {
        super::cmyk_jpeg::decode(budget, pdf, dict, data, width, height)?
    } else {
        let mut samples = super::super::decode_zlib(data, budget)?;
        super::super::image_predictor::apply(budget, pdf, dict, &mut samples, length)?;
        samples
    };
    if samples.len() != length {
        return None;
    }
    let mapping = SampleDecode::read_cmyk(budget, pdf, dict)?;
    let mask = if super::super::image_smask::is_present(dict) {
        let text = crate::metadata_input::text(budget, dict)?;
        Some(super::super::image_smask::decode_with_verified_source(
            budget, pdf, &text, width, height,
        )?)
    } else {
        None
    };
    let mut rgb = budget.image_vector(count.checked_mul(3)?)?;
    let mut input = [0.0f64; 1024];
    let mut output = [0.0f64; 768];
    for (chunk_index, chunk) in samples.chunks(1024).enumerate() {
        if budget.is_stopped() {
            return None;
        }
        for (index, &sample) in chunk.iter().enumerate() {
            let value = mapping.rgb_component(sample, index % 4) / 255.0;
            input[index] = mask.as_ref().map_or(value, |mask| {
                mask.source_component(value, chunk_index * 256 + index / 4, index % 4)
            });
        }
        let output = &mut output[..chunk.len() / 4 * 3];
        transform.transform(&input[..chunk.len()], output).ok()?;
        for &value in output.iter() {
            if !value.is_finite() {
                return None;
            }
            rgb.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    if let Some(mask) = mask {
        let rgba = super::super::image_pixels::with_alpha(budget, &rgb, &mask.alpha)?;
        super::super::encode_rgba8_png(budget, width, height, &rgba)
    } else {
        super::super::encode_rgb8_png(budget, width, height, &rgb)
    }
}
