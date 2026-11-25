//! 罫線gridの実際の間隔から、表示行順の高さを保持する。

/// 平均行高へ潰さず、薄い罫線の既存補正だけを各行へ分配する。
pub(super) fn heights(ys: &[i32], extra_points: f32) -> Vec<f32> {
    let Some(count) = ys.len().checked_sub(1).filter(|count| *count > 0) else {
        return Vec::new();
    };
    if !extra_points.is_finite() || extra_points < 0.0 {
        return Vec::new();
    }
    let extra = f64::from(extra_points) / count as f64;
    let Some(heights) = ys
        .windows(2)
        .rev()
        .map(|pair| {
            let points = f64::from(pair[1]) - f64::from(pair[0]);
            let twips = ((points + extra) * 20.0).round();
            (points > 0.0 && twips.is_finite() && twips > 0.0 && twips <= f64::from(i32::MAX))
                .then_some(twips as f32)
        })
        .collect::<Option<Vec<_>>>()
    else {
        return Vec::new();
    };
    heights
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 罫線がない・逆順・ゼロ高・表現範囲外なら部分的な高さを公開しない。
    #[test]
    fn invalid_grid_or_adjustment_retains_fallback() {
        for ys in [&[][..], &[1], &[2, 2], &[3, 2], &[i32::MIN, i32::MAX]] {
            assert!(heights(ys, 0.0).is_empty());
        }
        for extra in [f32::NAN, f32::INFINITY, -1.0] {
            assert!(heights(&[0, 20, 100], extra).is_empty());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// 正の罫線間隔は個数・全高と、上から下の各行への対応を保つ。
        #[test]
        fn grid_height_is_conserved_with_only_twip_rounding(
            gaps in proptest::collection::vec(1u16..1000, 1..16),
            extra in 0u16..100,
        ) {
            let mut ys = vec![-100];
            for gap in &gaps { ys.push(ys.last().unwrap() + i32::from(*gap)); }
            let actual = heights(&ys, f32::from(extra) / 100.0);
            prop_assert_eq!(actual.len(), gaps.len());
            prop_assert!(actual.iter().all(|v| v.is_finite() && *v > 0.0));
            let total: f32 = actual.iter().sum();
            let expected = gaps.iter().map(|v| f32::from(*v)).sum::<f32>() * 20.0 + f32::from(extra) / 5.0;
            prop_assert!((total - expected).abs() <= gaps.len() as f32 * 0.5 + 0.1);
            for (height, gap) in actual.iter().zip(gaps.iter().rev()) {
                let expected = f32::from(*gap) * 20.0 + f32::from(extra) / 5.0 / gaps.len() as f32;
                prop_assert!((*height - expected).abs() <= 0.51);
            }
        }
    }
}
