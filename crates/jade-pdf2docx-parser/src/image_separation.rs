//! Separation画像をsource Decode・Matte・sampled tint・alternateの順で復元する。
use super::{DecodeBudget, ImageFilter, image_attribute, image_sample_decode::SampleDecode};
use moxcms::{BarycentricWeightScale, ColorProfile, Layout, TransformOptions};

#[path = "image_tint.rs"]
mod tint;

/// Separationの先頭名を解決し、未対応画像もlegacy codecへ流さない。
pub(super) fn is_separation(budget: &DecodeBudget, pdf: &[u8], dict: &[u8]) -> Option<bool> {
    if crate::metadata::value(dict, b"ColorSpace").is_none() {
        return Some(false);
    }
    let color = image_attribute(budget, pdf, dict, b"ColorSpace")?;
    Some(
        crate::metadata::array_entries(&color, 4)
            .and_then(|mut e| e.next())
            .flatten()
            .is_some_and(|first| crate::metadata::is_name(first, b"Separation")),
    )
}

/// 一成分Flateを有界に復号し、最終sRGBだけを8 bitへ量子化する。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if budget.is_stopped()
        || ImageFilter::read(dict)? != ImageFilter::Flate
        || super::image_sample_decode::number(
            budget,
            pdf,
            &image_attribute(budget, pdf, dict, b"BitsPerComponent")?,
        )? != 8.0
    {
        return None;
    }
    let color = image_attribute(budget, pdf, dict, b"ColorSpace")?;
    let mut entries = crate::metadata::array_entries(&color, 4)?;
    if !crate::metadata::is_name(entries.next()??, b"Separation") {
        return None;
    }
    let name = entries.next()??;
    let mut lexer = crate::syntax::Lexer {
        bytes: name,
        cursor: 0,
    };
    if !matches!(lexer.next(), Some(crate::syntax::Token::Name(_)))
        || lexer.next().is_some()
        || crate::metadata::is_name(name, b"All")
        || crate::metadata::is_name(name, b"None")
    {
        return None;
    }
    let alternate = entries.next()??;
    let function = entries.next()??;
    if entries.next().is_some() {
        return None;
    }
    let components = if crate::metadata::is_name(alternate, b"DeviceRGB") {
        3
    } else if crate::metadata::is_name(alternate, b"DeviceGray") {
        1
    } else {
        4
    };
    let transform = if components == 4 {
        let profile = super::image_icc::cmyk::read(budget, pdf, alternate)?;
        Some(
            profile
                .create_transform_f64(
                    Layout::Rgba,
                    &ColorProfile::new_srgb(),
                    Layout::Rgb,
                    TransformOptions {
                        barycentric_weight_scale: BarycentricWeightScale::High,
                        rendering_intent: super::image_icc::intent(budget, pdf, dict)?,
                        allow_use_cicp_transfer: false,
                        prefer_fixed_point: false,
                        ..Default::default()
                    },
                )
                .ok()?,
        )
    } else {
        None
    };
    let tint = tint::SampledTint::read(budget, pdf, function, components)?;
    let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if count == 0 {
        return None;
    }
    let mut samples = super::decode_zlib(data, budget)?;
    super::image_predictor::apply(budget, pdf, dict, &mut samples, count)?;
    if samples.len() != count {
        return None;
    }
    let mapping = SampleDecode::read_separation(budget, pdf, dict)?;
    let mask = if super::image_smask::is_present(dict) {
        let text = crate::metadata_input::text(budget, dict)?;
        Some(super::image_smask::decode_with_verified_source(
            budget, pdf, &text, width, height,
        )?)
    } else {
        None
    };
    let mut rgb = budget.image_vector(count.checked_mul(3)?)?;
    let mut input = [0.0f64; 1024];
    let mut output = [0.0f64; 768];
    for (chunk_index, chunk) in samples.chunks(256).enumerate() {
        if budget.is_stopped() {
            return None;
        }
        for (index, &sample) in chunk.iter().enumerate() {
            let value = mapping.rgb_component(sample, 0) / 255.0;
            let value = mask.as_ref().map_or(value, |mask| {
                mask.source_component(value, chunk_index * 256 + index, 0)
            });
            let alternate = tint.evaluate(value);
            if transform.is_some() {
                for (component, value) in input[index * 4..index * 4 + 4].iter_mut().zip(alternate)
                {
                    *component = value.clamp(0.0, 1.0);
                }
            } else if components == 1 {
                output[index * 3..index * 3 + 3].fill(alternate[0]);
            } else {
                output[index * 3..index * 3 + 3].copy_from_slice(&alternate[..3]);
            }
        }
        if let Some(transform) = &transform {
            transform
                .transform(&input[..chunk.len() * 4], &mut output[..chunk.len() * 3])
                .ok()?;
        }
        for &value in &output[..chunk.len() * 3] {
            if !value.is_finite() {
                return None;
            }
            rgb.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    if let Some(mask) = mask {
        let rgba = super::image_pixels::with_alpha(budget, &rgb, &mask.alpha)?;
        super::encode_rgba8_png(budget, width, height, &rgba)
    } else {
        super::encode_rgb8_png(budget, width, height, &rgb)
    }
}
