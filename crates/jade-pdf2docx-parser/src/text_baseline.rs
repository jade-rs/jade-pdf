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

/// 表示原点へ基線を移し、ページ回転後に正立と確認できない基線は公開しない。
pub(crate) fn in_viewport(baseline: Option<f32>, origin_y: f32, rotation: i32) -> Option<f32> {
    let y = baseline?;
    if rotation.rem_euclid(360) != 0 || !y.is_finite() || !origin_y.is_finite() {
        return None;
    }
    let relative = y - origin_y;
    relative.is_finite().then_some(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 非有限値・計算overflow・未知基線を、表示内の正立基線へ昇格しない。
    #[test]
    fn invalid_or_unconfirmed_baselines_remain_unknown() {
        assert_eq!(in_viewport(None, 0., 0), None);
        assert_eq!(in_viewport(Some(f32::NAN), 0., 0), None);
        assert_eq!(in_viewport(Some(80.), f32::INFINITY, 0), None);
        assert_eq!(in_viewport(Some(f32::MAX), -f32::MAX, 0), None);
        assert_eq!(in_viewport(Some(80.), 50., 45), None);
    }
}
