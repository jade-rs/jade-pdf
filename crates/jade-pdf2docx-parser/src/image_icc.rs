//! 対応する8 bitのICCBased画像をsource Decode・Matteの後でsRGBへ写像する。
use super::{DecodeBudget, ImageFilter, image_attribute, image_sample_decode::SampleDecode};
use moxcms::{ColorProfile, Layout, RenderingIntent, TransformOptions};

#[path = "image_icc_cmyk.rs"]
pub(super) mod cmyk;
#[path = "image_icc_cmyk_image.rs"]
mod cmyk_image;
#[path = "image_icc_cmyk_jpeg.rs"]
mod cmyk_jpeg;
#[path = "image_icc_dct.rs"]
mod dct;
#[path = "image_icc_gray.rs"]
mod gray;
#[path = "image_icc_profile.rs"]
mod profile;

/// 空白・コメント以外に余分なtokenがないnullを検証する。
pub(super) fn is_null(bytes: &[u8]) -> bool {
    let mut lexer = crate::syntax::Lexer { bytes, cursor: 0 };
    matches!(lexer.next(), Some(crate::syntax::Token::Word(b"null"))) && lexer.next().is_none()
}

/// 対応する画像Intentを読む。未指定時はPDFの既定値を用いる。
pub(super) fn intent(budget: &DecodeBudget, pdf: &[u8], dict: &[u8]) -> Option<RenderingIntent> {
    if crate::metadata::value(dict, b"Intent").is_none() {
        return Some(RenderingIntent::RelativeColorimetric);
    }
    let value = image_attribute(budget, pdf, dict, b"Intent")?;
    [
        (b"Perceptual".as_slice(), RenderingIntent::Perceptual),
        (
            b"RelativeColorimetric",
            RenderingIntent::RelativeColorimetric,
        ),
        (
            b"AbsoluteColorimetric",
            RenderingIntent::AbsoluteColorimetric,
        ),
        (b"Saturation", RenderingIntent::Saturation),
    ]
    .into_iter()
    .find_map(|(name, intent)| crate::metadata::is_name(&value, name).then_some(intent))
}

/// 色空間配列の先頭だけを解決し、未対応ICCもlegacy JPEGへ流さない。
pub(super) fn is_icc(budget: &DecodeBudget, pdf: &[u8], dict: &[u8]) -> Option<bool> {
    if crate::metadata::value(dict, b"ColorSpace").is_none() {
        return Some(false);
    }
    let color = image_attribute(budget, pdf, dict, b"ColorSpace")?;
    Some(
        crate::metadata::array_entries(&color, 3)
            .and_then(|mut entries| entries.next())
            .flatten()
            .is_some_and(|first| crate::metadata::is_name(first, b"ICCBased")),
    )
}

/// ICCBasedの配列長・profile参照・指定したNを検証する。
fn dictionary<'a>(
    budget: &DecodeBudget,
    pdf: &'a [u8],
    dict: &[u8],
    components: usize,
) -> Option<(&'a [u8], &'a [u8])> {
    let color = image_attribute(budget, pdf, dict, b"ColorSpace")?;
    let mut entries = crate::metadata::array_entries(&color, 2)?;
    if !crate::metadata::is_name(entries.next()??, b"ICCBased") {
        return None;
    }
    let object = budget.reference(pdf, entries.next()??)?;
    if entries.next().is_some() {
        return None;
    }
    let (dict, data) = super::extract_stream_for_object(budget, pdf, object)?;
    if super::image_sample_decode::number(budget, pdf, &image_attribute(budget, pdf, dict, b"N")?)?
        != components as f64
    {
        return None;
    }
    Some((dict, data))
}

/// ICCBased配列とprofile辞書から、対応するsource成分数を読む。
pub(super) fn components(budget: &DecodeBudget, pdf: &[u8], dict: &[u8]) -> Option<usize> {
    let color = image_attribute(budget, pdf, dict, b"ColorSpace")?;
    let mut entries = crate::metadata::array_entries(&color, 2)?;
    if !crate::metadata::is_name(entries.next()??, b"ICCBased") {
        return None;
    }
    let object = budget.reference(pdf, entries.next()??)?;
    if entries.next().is_some() {
        return None;
    }
    let (profile, _) = super::extract_stream_for_object(budget, pdf, object)?;
    let n = super::image_sample_decode::number(
        budget,
        pdf,
        &image_attribute(budget, pdf, profile, b"N")?,
    )?;
    match n {
        1.0 => Some(1),
        3.0 => Some(3),
        4.0 => Some(4),
        _ => None,
    }
}

/// sourceの寸法・成分を照合し、RGBの最終量子化後にalphaを付ける。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if budget.is_stopped()
        || super::image_sample_decode::number(
            budget,
            pdf,
            &image_attribute(budget, pdf, dict, b"BitsPerComponent")?,
        )? != 8.0
    {
        return None;
    }
    match components(budget, pdf, dict)? {
        1 => return gray::decode(budget, pdf, dict, data, width, height),
        4 => return cmyk_image::decode(budget, pdf, dict, data, width, height),
        _ => {}
    }
    let (profile_dict, profile_data) = dictionary(budget, pdf, dict, 3)?;
    let source = profile::read(budget, pdf, profile_dict, profile_data)?;
    let intent = intent(budget, pdf, dict)?;
    let transform = source
        .create_transform_f64(
            Layout::Rgb,
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
    if budget.is_stopped() {
        return None;
    }
    let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    let length = count.checked_mul(3)?;
    let filter = ImageFilter::read(dict)?;
    let finish = |rgb: &mut [u8]| -> Option<Vec<u8>> {
        if rgb.len() != length {
            return None;
        }
        let mapping = SampleDecode::read_rgb(budget, pdf, dict)?;
        let mask = if super::image_smask::is_present(dict) {
            let text = crate::metadata_input::text(budget, dict)?;
            Some(super::image_smask::decode_with_verified_source(
                budget, pdf, &text, width, height,
            )?)
        } else {
            None
        };
        // 全画像の浮動小数点配列を作らず、固定長chunkごとに停止要求も検査する。
        let mut input = [0.0f64; 768];
        let mut output = [0.0f64; 768];
        for (chunk_index, chunk) in rgb.chunks_mut(768).enumerate() {
            if budget.is_stopped() {
                return None;
            }
            for (index, &sample) in chunk.iter().enumerate() {
                let value = mapping.rgb_component(sample, index % 3) / 255.0;
                input[index] = mask.as_ref().map_or(value, |mask| {
                    mask.source_component(value, chunk_index * 256 + index / 3, index % 3)
                });
            }
            transform
                .transform(&input[..chunk.len()], &mut output[..chunk.len()])
                .ok()?;
            for (sample, &value) in chunk.iter_mut().zip(&output) {
                if !value.is_finite() {
                    return None;
                }
                *sample = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        if let Some(mask) = mask {
            let rgba = super::image_pixels::with_alpha(budget, rgb, &mask.alpha)?;
            super::encode_rgba8_png(budget, width, height, &rgba)
        } else if filter == ImageFilter::Dct {
            super::image_png::encode_jpeg(budget, width, height, rgb)
        } else {
            super::encode_rgb8_png(budget, width, height, rgb)
        }
    };
    match filter {
        ImageFilter::Dct => super::with_dct_jpeg_rgb(
            budget,
            data,
            |decoded_width, decoded_height, components, rgb| {
                if components != 3 || (decoded_width, decoded_height) != (width, height) {
                    return None;
                }
                finish(rgb)
            },
        ),
        ImageFilter::Flate => {
            let mut samples = super::decode_zlib(data, budget)?;
            super::image_predictor::apply(budget, pdf, dict, &mut samples, length)?;
            finish(&mut samples)
        }
        _ => None,
    }
}
