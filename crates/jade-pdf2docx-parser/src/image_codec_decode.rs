//! RGBへ復元できるcodecのsource Decodeを、MatteとPNG量子化の前に適用する。
use super::{DecodeBudget, ImageFilter, image_sample_decode::SampleDecode};

/// codecのsource成分数と、既存の保存予算へ予約済みのRGB画素。
pub(super) struct Image {
    /// codecが宣言した幅。
    pub(super) width: u32,
    /// codecが宣言した高さ。
    pub(super) height: u32,
    /// JPEGまたはJBIG2のsource成分数。
    pub(super) components: usize,
    /// codec復元後・Decode写像前のRGB画素。
    pub(super) rgb: Vec<u8>,
}

/// 色成分の対応と寸法を検証し、source Decode・Matteの順で合成する。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
    filter: ImageFilter,
) -> Option<Vec<u8>> {
    let (mut image, bits) = match filter {
        ImageFilter::Dct => (super::decode_dct_jpeg_rgb(budget, data)?, 8),
        ImageFilter::Jbig2 => {
            let text = crate::metadata_input::text(budget, dictionary)?;
            let decoded = super::decode_jbig2_rgb(budget, pdf, &text, data, 0, 0)?;
            (
                Image {
                    width: decoded.width,
                    height: decoded.height,
                    components: 1,
                    rgb: decoded.rgb,
                },
                1,
            )
        }
        _ => return None,
    };
    if (image.width, image.height) != (width, height)
        || image.rgb.len()
            != usize::try_from(u64::from(width) * u64::from(height))
                .ok()?
                .checked_mul(3)?
    {
        return None;
    }
    let mapping = SampleDecode::read_codec(budget, pdf, dictionary, image.components, bits)?;
    if super::image_smask::is_present(dictionary) {
        let text = crate::metadata_input::text(budget, dictionary)?;
        let mask =
            super::image_smask::decode_with_verified_source(budget, pdf, &text, width, height)?;
        let mut rgba = super::image_pixels::with_alpha(budget, &image.rgb, &mask.alpha)?;
        mask.apply_source_decode(budget, &mut rgba, &mapping)?;
        super::encode_rgba8_png(budget, width, height, &rgba)
    } else {
        mapping.apply(budget, &mut image.rgb)?;
        super::encode_rgb8_png(budget, width, height, &image.rgb)
    }
}
