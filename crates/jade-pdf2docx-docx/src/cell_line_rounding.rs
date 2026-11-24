//! 一つの表セル内のpoint距離を累積し、twipsの各末端を量子化して丸め誤差を持ち越す。

/// 原本の非負の距離と出力済みの整数末端を保持する。無効な入力で状態を進めない。
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Accumulator {
    /// セル上端から累積した、まだ丸めないpoint距離。
    points: f64,
    /// 最後に正常に出力した行boxの末端。
    twips: u64,
}

impl Accumulator {
    /// 前余白とem高の各末端を丸め、前余白・正の行高・累積末端をDOCXの整数範囲で返す。
    pub(super) fn line(&mut self, before: f32, height: f32) -> Option<(u32, i32, u64)> {
        if !before.is_finite() || before < 0. || !height.is_finite() || height <= 0. {
            return None;
        }
        let start_points = self.points + f64::from(before);
        let end_points = start_points + f64::from(height);
        let start = endpoint(start_points)?;
        let end = endpoint(end_points)?;
        let before = u32::try_from(start.checked_sub(self.twips)?).ok()?;
        let height = i32::try_from(end.checked_sub(start)?).ok()?;
        if height <= 0 {
            return None;
        }
        self.points = end_points;
        self.twips = end;
        Some((before, height, end))
    }
}

/// 非有限・負・整数範囲外の距離を、飽和castによる有効な末端へ置き換えない。
fn endpoint(points: f64) -> Option<u64> {
    let twips = (points * 20.).round();
    (twips.is_finite() && twips >= 0. && twips < u64::MAX as f64).then_some(twips as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 壊れた行や整数にできない高さを挟んでも、その後の行の誤差を変えない。
    #[test]
    fn invalid_distances_do_not_advance_the_accumulator() {
        let mut state = Accumulator::default();
        assert!(state.line(3.45, 7.98).is_some());
        let valid = state.clone();
        for (before, height) in [
            (f32::NAN, 8.),
            (f32::INFINITY, 8.),
            (-1., 8.),
            (0., f32::NAN),
            (0., f32::INFINITY),
            (0., -1.),
            (0., 0.),
            (0., 0.001),
            (f32::MAX, 8.),
            (0., f32::MAX),
        ] {
            assert!(state.line(before, height).is_none(), "{before} {height}");
            assert_eq!(state, valid);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]
        /// 小数pointの前余白・高さを繰り返しても、出力末端は独立した整数千分point基準から1twip以内に収まる。
        #[test]
        fn endpoints_do_not_accumulate_per_segment_rounding(
            rows in prop::collection::vec((0u32..10000, 1000u32..20000), 1..129)
        ) {
            let mut state = Accumulator::default();
            let mut source_milli = 0u64;
            let mut actual = 0u64;
            for (before, height) in rows {
                let (blank, line, end) = state.line(before as f32 / 1000., height as f32 / 1000.).unwrap();
                prop_assert!(line > 0);
                actual += u64::from(blank) + line as u64;
                prop_assert_eq!(actual, end);
                source_milli += u64::from(before) + u64::from(height);
                let expected = (source_milli * 20 + 500) / 1000;
                prop_assert!((actual as i64 - expected as i64).abs() <= 1);
            }
        }
    }
}
