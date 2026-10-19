//! 外部soft maskを必須の透明度として復号し、失敗時は元画像を拒否する。
use super::{
    DecodeBudget, ImageFilter, decode_filter0_png_to_rgba, decode_zlib, encode_rgba8_png,
    extract_stream_for_object, image_attribute, image_ccitt, image_decode_parameters, image_pixels,
    image_predictor, image_sample_decode::SampleDecode,
};

#[path = "image_matte.rs"]
mod matte;

/// 復号した透明度と、source色空間に属する任意のMatte。
pub(super) struct SoftMask {
    /// 元画像と同じ個数の透明度。
    pub(super) alpha: Vec<u8>,
    /// 最大四成分のsource色に属するMatte。Grayは各成分へ展開する。
    matte: Option<[f64; 4]>,
}

impl SoftMask {
    /// 透明度を適用し、Matteありなら色変換前の成分を逆変換する。
    pub(super) fn apply(&self, budget: &DecodeBudget, rgba: &mut [u8]) -> Option<()> {
        self.apply_components(budget, rgba, None)
    }

    /// source DecodeをMatteの前に適用し、最終色だけを整数へ丸める。
    pub(super) fn apply_source_decode(
        &self,
        budget: &DecodeBudget,
        rgba: &mut [u8],
        decode: &SampleDecode,
    ) -> Option<()> {
        self.apply_components(budget, rgba, Some(decode))
    }

    /// ICC変換の前に、正規化source成分のMatteを整数化せず逆変換する。
    pub(super) fn source_component(&self, value: f64, pixel: usize, channel: usize) -> f64 {
        if let Some(matte) = self.matte {
            let alpha = self.alpha[pixel];
            if alpha == 0 {
                0.0
            } else {
                (matte[channel] + (value - matte[channel]) * 255.0 / f64::from(alpha))
                    .clamp(0.0, 1.0)
            }
        } else {
            value
        }
    }

    /// 復元済み成分または未写像sampleへ透明度とMatteを合成する。
    fn apply_components(
        &self,
        budget: &DecodeBudget,
        rgba: &mut [u8],
        decode: Option<&SampleDecode>,
    ) -> Option<()> {
        if rgba.len() != self.alpha.len().checked_mul(4)? || budget.is_stopped() {
            return None;
        }
        for (index, (pixel, &alpha)) in rgba.chunks_exact_mut(4).zip(&self.alpha).enumerate() {
            if index % 4096 == 0 && budget.is_stopped() {
                return None;
            }
            for (channel, component) in pixel[..3].iter_mut().enumerate() {
                let value = decode.map_or(f64::from(*component), |decode| {
                    decode.rgb_component(*component, channel)
                });
                let value = if let Some(matte) = self.matte {
                    if alpha == 0 {
                        0.0
                    } else {
                        let matte = matte[channel] * 255.0;
                        matte + (value - matte) * 255.0 / f64::from(alpha)
                    }
                } else {
                    value
                };
                *component = value.round().clamp(0.0, 255.0) as u8;
            }
            pixel[3] = alpha;
        }
        (!budget.is_stopped()).then_some(())
    }
}

/// 未指定またはnull以外のSMaskは、透明度の復号が必要である。
pub(super) fn is_present(dictionary: &[u8]) -> bool {
    crate::metadata::value(dictionary, b"SMask").is_some_and(|value| value != b"null")
}

/// Matte付きsoft maskは、codecのsource色を検証してから合成する必要がある。
pub(super) fn requires_source_color(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
) -> Option<bool> {
    if !is_present(dictionary) {
        return Some(false);
    }
    let object = crate::metadata::value(dictionary, b"SMask")
        .and_then(|value| budget.reference(pdf, value))?;
    let (mask, _) = extract_stream_for_object(budget, pdf, object)?;
    if crate::metadata::value(mask, b"Matte").is_none() {
        return Some(false);
    }
    let matte = image_attribute(budget, pdf, mask, b"Matte")?;
    Some(single_word(&matte) != Some(b"null"))
}

/// 自前codecのPNGへmaskを適用し、指定がなければ既存配列を返す。
pub(super) fn apply_to_png(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    png: Vec<u8>,
) -> Option<Vec<u8>> {
    if budget.is_stopped() {
        return None;
    }
    if !is_present(dictionary) {
        return Some(png);
    }
    let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    let text = crate::metadata_input::text(budget, dictionary)?;
    let mask = decode(budget, pdf, &text, width, height)?;
    let mut image = decode_filter0_png_to_rgba(budget, &png)?;
    mask.apply(budget, &mut image.rgba)?;
    encode_rgba8_png(budget, image.width, image.height, &image.rgba)
}

/// DeviceGrayの8 bit Flate/1 bit CCITT maskを復号し、Decode反転と寸法変換を適用する。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary_text: &str,
    width: u32,
    height: u32,
) -> Option<SoftMask> {
    decode_inner(budget, pdf, dictionary_text, width, height, false)
}

/// codecの成分数と寸法を検証済みの画像に限りMatteを許可する。
pub(super) fn decode_with_verified_source(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary_text: &str,
    width: u32,
    height: u32,
) -> Option<SoftMask> {
    decode_inner(budget, pdf, dictionary_text, width, height, true)
}

/// source成分の検証状態を保持してmask属性と画素を復号する。
fn decode_inner(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary_text: &str,
    width: u32,
    height: u32,
    source_components_verified: bool,
) -> Option<SoftMask> {
    if budget.is_stopped() {
        return None;
    }
    let object = crate::metadata::value(dictionary_text.as_bytes(), b"SMask")
        .and_then(|value| budget.reference(pdf, value))?;
    let (dictionary, data) = extract_stream_for_object(budget, pdf, object)?;
    let mask_width = dimension(budget, pdf, dictionary, b"Width")?;
    let mask_height = dimension(budget, pdf, dictionary, b"Height")?;
    let bits = dimension(budget, pdf, dictionary, b"BitsPerComponent")?;
    let color = image_attribute(budget, pdf, dictionary, b"ColorSpace")?;
    let filter = ImageFilter::read(dictionary)?;
    if !crate::metadata::is_name(&color, b"DeviceGray")
        || !matches!(
            (bits, filter),
            (8, ImageFilter::Flate) | (1, ImageFilter::Ccitt)
        )
    {
        return None;
    }
    let matte = matte::read(
        budget,
        pdf,
        dictionary_text.as_bytes(),
        dictionary,
        source_components_verified,
    )?;
    if matte.is_some() && (mask_width != width || mask_height != height) {
        return None;
    }
    if [b"Mask".as_slice(), b"SMask"]
        .iter()
        .any(|key| crate::metadata::value(dictionary, key).is_some_and(|value| value != b"null"))
    {
        return None;
    }
    if crate::metadata::value(dictionary, b"ImageMask").is_some() {
        let value = image_attribute(budget, pdf, dictionary, b"ImageMask")?;
        if !matches!(single_word(&value)?, b"false" | b"null") {
            return None;
        }
    }
    let inverted = inverted_decode(budget, pdf, dictionary)?;
    budget.check_image_pixels(mask_width, mask_height)?;
    let count = mask_width.checked_mul(mask_height)? as usize;
    let decoded = if filter == ImageFilter::Ccitt {
        let parameters = image_decode_parameters(budget, pdf, dictionary)?;
        image_ccitt::decode_gray(budget, pdf, data, mask_width, mask_height, &parameters)?
    } else {
        let mut decoded = decode_zlib(data, budget)?;
        image_predictor::apply(budget, pdf, dictionary, &mut decoded, count)?;
        decoded
    };
    let mut alpha = image_pixels::resample_gray(
        budget,
        decoded.get(..count)?,
        mask_width,
        mask_height,
        width,
        height,
    )?;
    if inverted {
        for pixel in &mut alpha {
            *pixel = 255 - *pixel;
        }
    }
    Some(SoftMask { alpha, matte })
}

/// 配列容量境界のunit test向けに透明度だけを返す。
#[cfg(test)]
pub(super) fn decode_alpha(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &str,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    Some(decode(budget, pdf, dictionary, width, height)?.alpha)
}

/// maskの整数属性を正のu32として読み、端数やゼロを丸めない。
fn dimension(budget: &DecodeBudget, pdf: &[u8], dictionary: &[u8], key: &[u8]) -> Option<u32> {
    let value = image_attribute(budget, pdf, dictionary, key)?;
    let number = std::str::from_utf8(single_word(&value)?)
        .ok()?
        .parse::<u32>()
        .ok()?;
    (number > 0).then_some(number)
}

/// 間接objectの空白とコメントを許し、単一のwordだけを読む。
fn single_word(bytes: &[u8]) -> Option<&[u8]> {
    let mut lexer = crate::syntax::Lexer { bytes, cursor: 0 };
    let crate::syntax::Token::Word(word) = lexer.next()? else {
        return None;
    };
    lexer.next().is_none().then_some(word)
}

/// 対応するidentity/反転のDecode配列だけを借用走査し、他の指定を拒否する。
fn inverted_decode(budget: &DecodeBudget, pdf: &[u8], dictionary: &[u8]) -> Option<bool> {
    let Some(value) = crate::metadata::value(dictionary, b"Decode") else {
        return Some(false);
    };
    if value == b"null" {
        return Some(false);
    }
    let value = image_attribute(budget, pdf, dictionary, b"Decode")?;
    let mut entries = crate::metadata::array_entries(&value, 2)?;
    let first = std::str::from_utf8(entries.next()??)
        .ok()?
        .parse::<f32>()
        .ok()?;
    let second = std::str::from_utf8(entries.next()??)
        .ok()?
        .parse::<f32>()
        .ok()?;
    if entries.next().is_some() {
        return None;
    }
    match (first, second) {
        (0.0, 1.0) => Some(false),
        (1.0, 0.0) => Some(true),
        _ => None,
    }
}
