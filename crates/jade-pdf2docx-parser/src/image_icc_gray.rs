//! D50 Gray TRCの8 bit Flate/JPEG画像をsource色のまま補正してsRGBへ変換する。
use super::super::{DecodeBudget, ImageFilter, image_sample_decode::SampleDecode};
use moxcms::{ColorProfile, Layout, RenderingIntent, TransformOptions};

/// 一成分Decode・Matte・ICC変換の途中で丸めず、最終RGBにalphaを付ける。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let intent = super::intent(budget, pdf, dict)?;
    // GrayのCMS経路はAbsoluteColorimetricの白色点変換を実装していない。
    if intent == RenderingIntent::AbsoluteColorimetric {
        return None;
    }
    let (profile_dict, profile_data) = super::dictionary(budget, pdf, dict, 1)?;
    let profile = super::profile::read_gray(budget, pdf, profile_dict, profile_data)?;
    let transform = profile
        .create_transform_f64(
            Layout::Gray,
            &ColorProfile::new_srgb(),
            Layout::Rgb,
            TransformOptions {
                rendering_intent: intent,
                allow_use_cicp_transfer: false,
                prefer_fixed_point: false,
                ..Default::default()
            },
        )
        .ok()?;
    let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if count == 0 || budget.is_stopped() {
        return None;
    }
    let samples = match ImageFilter::read(dict)? {
        ImageFilter::Dct => super::dct::decode_gray(budget, pdf, dict, data, width, height)?,
        ImageFilter::Flate => {
            let mut samples = super::super::decode_zlib(data, budget)?;
            super::super::image_predictor::apply(budget, pdf, dict, &mut samples, count)?;
            samples
        }
        _ => return None,
    };
    if samples.len() != count {
        return None;
    }
    let mapping = SampleDecode::read_gray(budget, pdf, dict)?;
    let mask = if super::super::image_smask::is_present(dict) {
        let text = crate::metadata_input::text(budget, dict)?;
        Some(super::super::image_smask::decode_with_verified_source(
            budget, pdf, &text, width, height,
        )?)
    } else {
        None
    };
    let mut rgb = budget.image_vector(count.checked_mul(3)?)?;
    let mut input = [0.0f64; 256];
    let mut output = [0.0f64; 768];
    for (chunk_index, chunk) in samples.chunks(256).enumerate() {
        if budget.is_stopped() {
            return None;
        }
        for (index, &sample) in chunk.iter().enumerate() {
            let value = mapping.rgb_component(sample, 0) / 255.0;
            input[index] = mask.as_ref().map_or(value, |mask| {
                mask.source_component(value, chunk_index * 256 + index, 0)
            });
        }
        let output = &mut output[..chunk.len() * 3];
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
