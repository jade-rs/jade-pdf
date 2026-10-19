//! 画像固有のalphaへ描画時の透明度を適用し、stencilだけ塗り色を置換する。
use crate::decode::DecodeBudget;

/// 予算内のRGBA bufferでalphaを一度だけ丸め、再符号化する。
pub(super) fn apply(
    budget: &DecodeBudget,
    png: &[u8],
    color: Option<[u8; 3]>,
    opacity: f64,
) -> Option<Vec<u8>> {
    let mut image = super::decode_filter0_png_to_rgba(budget, png)?;
    for (index, pixel) in image.rgba.chunks_exact_mut(4).enumerate() {
        if index % 4096 == 0 && budget.is_stopped() {
            return None;
        }
        if let Some(color) = color {
            pixel[..3].copy_from_slice(&color);
        }
        pixel[3] = (f64::from(pixel[3]) * opacity).round() as u8;
    }
    super::encode_rgba8_png(budget, image.width, image.height, &image.rgba)
}
