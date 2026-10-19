//! 1 bit stencilを黒とalphaへ復号し、配置ごとの塗り色を透明度と独立に適用する。
use super::{DecodeBudget, ImageFilter, image_attribute, image_sample_decode::SampleDecode};

/// 直接・間接のImageMaskを解決し、不正な型を画像復号失敗へ伝える。
pub(super) fn is_mask(budget: &DecodeBudget, pdf: &[u8], dictionary: &[u8]) -> Option<bool> {
    if crate::metadata::value(dictionary, b"ImageMask").is_none() {
        return Some(false);
    }
    let value = image_attribute(budget, pdf, dictionary, b"ImageMask")?;
    let mut lexer = crate::syntax::Lexer {
        bytes: &value,
        cursor: 0,
    };
    let result = match lexer.next()? {
        crate::syntax::Token::Word(b"true") => true,
        crate::syntax::Token::Word(b"false" | b"null") => false,
        _ => return None,
    };
    lexer.next().is_none().then_some(result)
}

/// Flate/CCITT/JBIG2の0/1 sampleをmask Decodeでalphaへ写す。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    for name in [b"ColorSpace".as_slice(), b"Mask", b"SMask"] {
        if crate::metadata::value(dictionary, name).is_some() {
            return None;
        }
    }
    let inverse = SampleDecode::stencil_inverse(budget, pdf, dictionary)?;
    let count = usize::try_from(width.checked_mul(height)?).ok()?;
    let filter = ImageFilter::read(dictionary)?;
    let (samples, packed) = match filter {
        ImageFilter::Flate => {
            let mut samples = super::decode_zlib(data, budget)?;
            let stride = usize::try_from(width).ok()?.checked_add(7)? / 8;
            let expected = stride.checked_mul(height as usize)?;
            super::image_predictor::apply(budget, pdf, dictionary, &mut samples, expected)?;
            if samples.len() != expected {
                return None;
            }
            (samples, true)
        }
        ImageFilter::Ccitt => {
            let parameters = super::image_decode_parameters(budget, pdf, dictionary)?;
            (
                super::image_ccitt::decode_gray(budget, pdf, data, width, height, &parameters)?,
                false,
            )
        }
        ImageFilter::Jbig2 => (
            super::decode_jbig2_gray(budget, pdf, dictionary, data, width, height)?,
            false,
        ),
        _ => return None,
    };
    let stride = (width as usize).checked_add(7)? / 8;
    let mut rgba = budget.image_vector(count.checked_mul(4)?)?;
    for index in 0..count {
        if index % 4096 == 0 && budget.is_stopped() {
            return None;
        }
        let bit = if packed {
            let row = index / width as usize;
            let column = index % width as usize;
            samples[row * stride + column / 8] & (128 >> (column % 8)) != 0
        } else {
            *samples.get(index)? != 0
        };
        rgba.extend_from_slice(&[0, 0, 0, if bit == inverse { 255 } else { 0 }]);
    }
    super::encode_rgba8_png(budget, width, height, &rgba)
}
