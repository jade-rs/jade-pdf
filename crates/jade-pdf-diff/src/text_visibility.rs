//! 非生成SDK文字の描画modeを数え、Unicodeの対応付けとは独立した事実として保持する。

/// 既存の最大2,000,000文字のSDK走査内でだけ使う、割り当てを持たない集計。
#[derive(Default)]
pub(crate) struct Counter {
    /// mode3（非表示）またはmode7（clipのみ）だった非生成SDK文字record数。
    pub nonpainting: usize,
    /// 非生成recordのobjectまたは描画modeを取得できなかったか。
    pub unavailable: bool,
    /// 直前に問い合わせた生SDK object。走査中だけ保持し、外部へ返さない。
    last_object: usize,
    /// 同じobject内でのSDK呼出しを増やさないための描画mode。
    last_mode: i32,
}

impl Counter {
    /// 生成空白は除外し、同じobjectのmodeを再利用して各recordを一度だけ集計する。
    pub(crate) fn observe(&mut self, object: usize, generated: bool, mode: impl FnOnce() -> i32) {
        if generated {
            return;
        }
        if object == 0 {
            self.unavailable = true;
            return;
        }
        if object != self.last_object {
            self.last_mode = mode();
            self.last_object = object;
        }
        match self.last_mode {
            3 | 7 => self.nonpainting += 1,
            0..=7 => {}
            _ => self.unavailable = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// 生成recordだけでは非表示や取得失敗を推定せず、objectへの問い合わせも行わない。
    #[test]
    fn generated_records_do_not_invent_visibility_evidence() {
        let mut counter = Counter::default();
        for object in [0, 1, usize::MAX] {
            counter.observe(object, true, || {
                panic!("generated record must not query object")
            });
        }
        assert_eq!(counter.nonpainting, 0);
        assert!(!counter.unavailable);
        counter.observe(0, false, || panic!("null object must not be queried"));
        assert!(counter.unavailable);
    }

    /// SDKの8種を区別し、未知の値を可視として扱わず取得不能を残す。
    #[test]
    fn known_modes_and_unknown_values_have_distinct_meanings() {
        for (mode, nonpainting, unavailable) in [
            (0, 0, false),
            (1, 0, false),
            (2, 0, false),
            (3, 1, false),
            (4, 0, false),
            (5, 0, false),
            (6, 0, false),
            (7, 1, false),
            (-1, 0, true),
            (8, 0, true),
            (i32::MIN, 0, true),
            (i32::MAX, 0, true),
        ] {
            let mut counter = Counter::default();
            counter.observe(1, false, || mode);
            assert_eq!(
                (counter.nonpainting, counter.unavailable),
                (nonpainting, unavailable),
                "mode {mode}"
            );
        }
    }

    /// 生成空白を挟んでも同じobjectを再問い合わせせず、文字record数自体は保持する。
    #[test]
    fn generated_gaps_do_not_invalidate_stable_object_mode() {
        let mut counter = Counter::default();
        let calls = Cell::new(0);
        counter.observe(1, false, || {
            calls.set(calls.get() + 1);
            3
        });
        counter.observe(0, true, || panic!());
        counter.observe(1, false, || panic!("stable object was queried twice"));
        counter.observe(2, false, || {
            calls.set(calls.get() + 1);
            0
        });
        counter.observe(1, false, || {
            calls.set(calls.get() + 1);
            3
        });
        assert_eq!(calls.get(), 3);
        assert_eq!(counter.nonpainting, 3);
        assert!(!counter.unavailable);
    }

    proptest::proptest! {
        /// SDK状態が不変な任意のrecord列で、生成文字を数えず呼出し数を走査長以内に抑える。
        #[test]
        fn record_accounting_is_bounded_and_generated_independent(
            modes in proptest::array::uniform3(proptest::prop_oneof![
                0i32..8,
                proptest::strategy::Just(-1),
                proptest::num::i32::ANY,
            ]),
            records in proptest::collection::vec((0usize..4, proptest::bool::ANY), 0..512),
        ) {
            let mut counter = Counter::default();
            let calls = Cell::new(0usize);
            let mut expected_count = 0usize;
            let mut expected_unavailable = false;
            for &(object, generated) in &records {
                counter.observe(object, generated, || {
                    calls.set(calls.get() + 1);
                    modes[object - 1]
                });
                if !generated {
                    if object == 0 {
                        expected_unavailable = true;
                    } else {
                        let value = modes[object - 1];
                        if [3, 7].contains(&value) {
                            expected_count += 1;
                        }
                        if !(0..=7).contains(&value) {
                            expected_unavailable = true;
                        }
                    }
                }
            }
            proptest::prop_assert_eq!(counter.nonpainting, expected_count);
            proptest::prop_assert_eq!(counter.unavailable, expected_unavailable);
            proptest::prop_assert!(calls.get() <= records.iter()
                .filter(|(object, generated)| *object != 0 && !*generated).count());
        }
    }
}
