//! ICC画像のDCT属性を検証し、Gray sourceをRGBへ展開せず復号する。
use super::super::{DecodeBudget, image_attribute, image_decode_parameters};
use zune_jpeg::{
    JpegDecoder,
    zune_core::{colorspace::ColorSpace, options::DecoderOptions},
};

/// 直接・間接・nullのColorTransformを、PDF整数の0/1として読む。
pub(super) fn color_transform(budget: &DecodeBudget, pdf: &[u8], dict: &[u8]) -> Option<u8> {
    let parameters = image_decode_parameters(budget, pdf, dict)?;
    if crate::metadata::value(&parameters, b"ColorTransform").is_none() {
        return Some(0);
    }
    let value = image_attribute(budget, pdf, &parameters, b"ColorTransform")?;
    if super::is_null(&value) {
        return Some(0);
    }
    let mut lexer = crate::syntax::Lexer {
        bytes: &value,
        cursor: 0,
    };
    let crate::syntax::Token::Word(word) = lexer.next()? else {
        return None;
    };
    if lexer.next().is_some() {
        return None;
    }
    let number = std::str::from_utf8(word).ok()?.parse::<u8>().ok()?;
    (number <= 1).then_some(number)
}

/// 一成分のJPEG headerを照合し、予算内で8 bitのGray sourceだけを返す。
pub(super) fn decode_gray(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    // ColorTransformは一成分では適用しないが、属性の型と値は検証する。
    color_transform(budget, pdf, dict)?;
    let options = DecoderOptions::default()
        .set_max_width(u16::MAX.into())
        .set_max_height(u16::MAX.into())
        .jpeg_set_out_colorspace(ColorSpace::Luma);
    let mut decoder = JpegDecoder::new_with_options(data, options);
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    budget.check_image_pixels(info.width.into(), info.height.into())?;
    if (
        u32::from(info.width),
        u32::from(info.height),
        info.components,
    ) != (width, height, 1)
        || decoder.get_input_colorspace()? != ColorSpace::Luma
    {
        return None;
    }
    let length = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if decoder.output_buffer_size()? != length || budget.is_stopped() {
        return None;
    }
    let mut samples = budget.image_vector(length)?;
    samples.resize(length, 0);
    decoder.decode_into(&mut samples).ok()?;
    (decoder.get_output_colorspace()? == ColorSpace::Luma && !budget.is_stopped())
        .then_some(samples)
}
