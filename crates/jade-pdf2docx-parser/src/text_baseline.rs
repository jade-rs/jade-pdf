//! fontの種類から推測せず、ページ空間の正立描画行列から基線だけを確認する。

/// 有限かつ正の軸を持つ正立描画だけ、ページ空間の基線座標を返す。
pub(crate) fn upright(matrix: [f32; 6]) -> Option<f32> {
    (matrix.iter().all(|v| v.is_finite())
        && matrix[0] > 0.
        && matrix[3] > 0.
        && matrix[1] == 0.
        && matrix[2] == 0.)
        .then_some(matrix[5])
}

/// 基線差0.0001pt以内だけ基線を共有し、既知と未知は結合しない。
pub(crate) fn compatible(previous: Option<f32>, next: Option<f32>) -> bool {
    match (previous, next) {
        (None, None) => true,
        (Some(a), Some(b)) => (a - b).abs() <= 0.0001,
        _ => false,
    }
}
