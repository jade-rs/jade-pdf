//! 画像の色変換・alpha合成・リサンプルを累積保存容量内で行う。
use super::DecodeBudget;

/// Gray8をRGB8へ展開し、出力全容量を確保前に予約する。
pub(super) fn gray(budget: &DecodeBudget, gray: &[u8]) -> Option<Vec<u8>> {
    let mut rgb = budget.image_vector(gray.len().checked_mul(3)?)?;
    for &value in gray {
        rgb.extend_from_slice(&[value; 3]);
    }
    Some(rgb)
}

/// Indexed画素をpaletteからRGB8へ展開する。
pub(super) fn indexed(budget: &DecodeBudget, indices: &[u8], palette: &[u8]) -> Option<Vec<u8>> {
    let mut rgb = budget.image_vector(indices.len().checked_mul(3)?)?;
    for &index in indices {
        let offset = usize::from(index) * 3;
        rgb.extend_from_slice(palette.get(offset..offset + 3)?);
    }
    Some(rgb)
}

/// RGB8と同じ画素数のalphaをRGBA8へ合成する。
pub(super) fn with_alpha(budget: &DecodeBudget, rgb: &[u8], alpha: &[u8]) -> Option<Vec<u8>> {
    if rgb.len() != alpha.len().checked_mul(3)? {
        return None;
    }
    let mut rgba = budget.image_vector(alpha.len().checked_mul(4)?)?;
    for (pixel, &alpha) in rgb.chunks_exact(3).zip(alpha) {
        rgba.extend_from_slice(pixel);
        rgba.push(alpha);
    }
    Some(rgba)
}

/// Gray8を最近傍で指定寸法へリサンプルし、出力画素数と容量を制限する。
pub(super) fn resample_gray(
    budget: &DecodeBudget,
    gray: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Option<Vec<u8>> {
    if src_width == 0
        || src_height == 0
        || dst_width == 0
        || dst_height == 0
        || gray.len() < src_width.checked_mul(src_height)? as usize
    {
        return None;
    }
    budget.check_image_pixels(dst_width, dst_height)?;
    let mut resized = budget.image_vector(dst_width.checked_mul(dst_height)? as usize)?;
    for y in 0..dst_height {
        let src_y = u64::from(y) * u64::from(src_height) / u64::from(dst_height);
        for x in 0..dst_width {
            let src_x = u64::from(x) * u64::from(src_width) / u64::from(dst_width);
            resized.push(*gray.get((src_y * u64::from(src_width) + src_x) as usize)?);
        }
    }
    Some(resized)
}
