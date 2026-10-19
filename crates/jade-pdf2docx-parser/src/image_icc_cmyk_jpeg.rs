//! DCTの四成分をRGBへ潰さず復号し、PDFのColorTransformだけを適用する。
use super::super::DecodeBudget;
use zune_jpeg::{
    JpegDecoder,
    zune_core::{colorspace::ColorSpace, options::DecoderOptions},
};

/// SOS前のmarkerを長さで走査し、APP14に埋まる偽markerを解釈しない。
fn adobe_transform(budget: &DecodeBudget, data: &[u8]) -> Option<Option<u8>> {
    if data.get(..2)? != b"\xff\xd8" {
        return None;
    }
    let mut cursor = 2usize;
    let mut adobe = None;
    loop {
        if budget.is_stopped() || *data.get(cursor)? != 255 {
            return None;
        }
        while *data.get(cursor)? == 255 {
            if budget.is_stopped() {
                return None;
            }
            cursor = cursor.checked_add(1)?;
        }
        let marker = *data.get(cursor)?;
        cursor = cursor.checked_add(1)?;
        if marker == 0xda {
            return Some(adobe);
        }
        if matches!(marker, 0 | 1 | 0xd0..=0xd9) {
            return None;
        }
        let length = usize::from(u16::from_be_bytes(
            data.get(cursor..cursor.checked_add(2)?)?.try_into().ok()?,
        ));
        if length < 2 {
            return None;
        }
        let end = cursor.checked_add(length)?;
        let segment = data.get(cursor + 2..end)?;
        if marker == 0xee && segment.starts_with(b"Adobe") {
            if adobe.is_some() {
                return None;
            }
            adobe = Some(*segment.get(11)?);
        }
        cursor = end;
    }
}

/// Adobe markerを優先し、Decode/Matte前のCMYK四成分を確保予算内で返す。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let requested = super::dct::color_transform(budget, pdf, dict)?;
    let transform = match adobe_transform(budget, data)? {
        Some(0) => false,
        Some(2) => true,
        Some(_) => return None,
        None => requested == 1,
    };
    let options = DecoderOptions::default()
        .set_max_width(u16::MAX.into())
        .set_max_height(u16::MAX.into());
    let mut decoder = JpegDecoder::new_with_options(data, options);
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    budget.check_image_pixels(info.width.into(), info.height.into())?;
    if (
        u32::from(info.width),
        u32::from(info.height),
        info.components,
    ) != (width, height, 4)
    {
        return None;
    }
    let raw_space = decoder.get_input_colorspace()?;
    if !matches!(raw_space, ColorSpace::CMYK | ColorSpace::YCCK) {
        return None;
    }
    decoder.set_options(options.jpeg_set_out_colorspace(raw_space));
    let length = usize::try_from(u64::from(width) * u64::from(height))
        .ok()?
        .checked_mul(4)?;
    if decoder.output_buffer_size()? != length {
        return None;
    }
    let mut samples = budget.image_vector(length)?;
    samples.resize(length, 0);
    decoder.decode_into(&mut samples).ok()?;
    if decoder.get_input_colorspace()? != raw_space || budget.is_stopped() {
        return None;
    }
    if transform {
        for pixel in samples.chunks_exact_mut(4) {
            if budget.is_stopped() {
                return None;
            }
            let y = f64::from(pixel[0]);
            let cb = f64::from(pixel[1]) - 128.0;
            let cr = f64::from(pixel[2]) - 128.0;
            // JPEG YCCKはYCbCr→RGBの補色と、変換しない第四成分から成る。
            for (channel, value) in [
                y + 1.402 * cr,
                y - 0.344136 * cb - 0.714136 * cr,
                y + 1.772 * cb,
            ]
            .into_iter()
            .enumerate()
            {
                pixel[channel] = 255 - value.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Some(samples)
}
