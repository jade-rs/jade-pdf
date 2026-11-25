//! 表示用96dpi寸法と、切り抜きPNGに保存する元の画素密度を分離する。
use crate::ParsedBounds;

/// 逆変換した一pointの画素長から、回転・shear後の各軸の保存画素数を求める。
/// 巨大な値はu32の上限へ飽和させ、呼出側の画素数・保存容量検査へ必ず渡す。
pub(super) fn dimensions(
    matrix: [f32; 6],
    bounds: ParsedBounds,
    width: u32,
    height: u32,
) -> (u32, u32) {
    let [a, b, c, d, ..] = matrix.map(f64::from);
    let determinant = (a * d - b * c).abs();
    let x_density = (d * f64::from(width)).hypot(b * f64::from(height)) / determinant;
    let y_density = (c * f64::from(width)).hypot(a * f64::from(height)) / determinant;
    let count = |span: f64, density: f64| {
        let count = (span * density).round();
        if count.is_finite() {
            count.clamp(1., f64::from(u32::MAX)) as u32
        } else {
            u32::MAX
        }
    };
    (
        count(f64::from(bounds.x1) - f64::from(bounds.x0), x_density),
        count(f64::from(bounds.y1) - f64::from(bounds.y0), y_density),
    )
}
