//! CTM用固定配列の連続数値・clearを独立した可変配列モデルで確認する。
use super::*;
use proptest::prelude::*;

/// 6数値を超えた場合の最古の破棄とclear後の再利用を固定する。
#[test]
fn rolling_six_operands_and_clear() {
    let mut operands = Operands::default();
    for number in 1..=8 {
        operands.push(number as f32);
    }
    assert_eq!(operands.as_slice(), [3., 4., 5., 6., 7., 8.]);
    operands.clear();
    assert!(operands.as_slice().is_empty());
    operands.push(-1.);
    assert_eq!(operands.as_slice(), [-1.]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 任意の追加・clear列で最後の6値と有効長が可変配列モデルと一致する。
    #[test]
    fn fixed_operands_match_last_six_model(operations in prop::collection::vec(prop::option::of(any::<i16>()), 0..120)) {
        let mut operands = Operands::default();
        let mut expected = Vec::new();
        for operation in operations {
            match operation {
                Some(value) => {
                    expected.push(f32::from(value));
                    if expected.len() > 6 { expected.remove(0); }
                    operands.push(f32::from(value));
                }
                None => { expected.clear(); operands.clear(); }
            }
            prop_assert_eq!(operands.len(), expected.len());
            prop_assert_eq!(operands.as_slice(), expected.as_slice());
        }
    }
}
