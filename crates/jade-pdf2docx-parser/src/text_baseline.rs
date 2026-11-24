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
