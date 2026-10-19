//! 参照走査の借用字句・結果容量・停止・重複除去を独立モデルで検証する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 共通容量のみを制限し、用途別容量は既定値に保つ。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_allocation_bytes: limit,
        ..Default::default()
    })
}

/// エラー取得後も走査を再開せず、最初の予約量を報告する。
fn exceeded(budget: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "total allocation bytes", limit: found, actual: total,
    }) if found == limit && total == actual));
    assert!(budget.is_stopped());
    assert!(parse_tounicode_object_refs(budget, b"/ToUnicode 1 0 R").is_empty());
}

/// 最初の四slotのinclusive境界を使い、token文字列は容量を消費しない。
#[test]
fn reference_allocation_first_capacity_boundary() {
    let total = 4 * std::mem::size_of::<u32>() as u64;
    let source = b"/ToUnicode [13] (0) /R /ToUnicode <2> <0> R /ToUnicode 13 0 R";
    let allowed = budget(total);
    assert_eq!(parse_tounicode_object_refs(&allowed, source), [2, 13]);
    assert!(allowed.check().is_ok());
    assert!(allowed.try_vector_capacity::<u8>(1).is_none());
    exceeded(&allowed, total, total + 1);
    let denied = budget(total - 1);
    assert!(parse_tounicode_object_refs(&denied, source).is_empty());
    exceeded(&denied, total - 1, total);
}

/// 五件目の全配列再確保を累積し、失敗時は先行参照も破棄する。
#[test]
fn reference_allocation_growth_discards_partial_results() {
    let source =
        b"/ToUnicode 5 0 R /ToUnicode 4 0 R /ToUnicode 3 0 R /ToUnicode 2 0 R /ToUnicode 1 0 R";
    let total = 12 * std::mem::size_of::<u32>() as u64;
    let allowed = budget(total);
    assert_eq!(
        parse_tounicode_object_refs(&allowed, source),
        [1, 2, 3, 4, 5]
    );
    assert!(allowed.check().is_ok());
    let denied = budget(total - 1);
    assert!(parse_tounicode_object_refs(&denied, source).is_empty());
    exceeded(&denied, total - 1, total);
}

/// 結果を解放しても予約は戻らず、再走査は同じ予算を消費する。
#[test]
fn reference_allocation_recreation_is_cumulative() {
    let total = 8 * std::mem::size_of::<u32>() as u64;
    let budget = budget(total - 1);
    assert_eq!(
        parse_tounicode_object_refs(&budget, b"/ToUnicode 1 0 R"),
        [1]
    );
    assert!(parse_tounicode_object_refs(&budget, b"/ToUnicode 1 0 R").is_empty());
    exceeded(&budget, total - 1, total);
}

/// 無効な十進参照はpanicせず除外し、後続の有効参照を読む。
#[test]
fn reference_allocation_invalid_tokens_are_borrowed_and_rejected() {
    let source = b"/ToUnicode ! 0 R /ToUnicode 12A 0 R /ToUnicode [] 0 R /ToUnicode 1 [] R /ToUnicode 4294967296 0 R /ToUnicode \xff 0 R /ToUnicode 7 0 R";
    let allowed = budget(4 * std::mem::size_of::<u32>() as u64);
    assert_eq!(parse_tounicode_object_refs(&allowed, source), [7]);
    assert!(allowed.check().is_ok());
}

/// 長い無効tokenと不完全参照は共通容量0でも配列を確保しない。
#[test]
fn reference_allocation_empty_scan_needs_no_capacity() {
    let source = format!("/ToUnicode {} 0 R /ToUnicode 1", "9".repeat(100_000));
    let budget = budget(0);
    assert!(parse_tounicode_object_refs(&budget, source.as_bytes()).is_empty());
    assert!(budget.check().is_ok());
}

/// metadata用途別上限も維持し、取得済みエラー後は無効参照の走査も再開しない。
#[test]
fn reference_allocation_metadata_limit_and_sticky_stop() {
    let total = 4 * std::mem::size_of::<u32>() as u64;
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: total - 1,
        ..Default::default()
    });
    assert!(parse_tounicode_object_refs(&budget, b"/ToUnicode 1 0 R").is_empty());
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit, actual,
    }) if limit == total - 1 && actual == total));
    assert!(parse_tounicode_object_refs(&budget, b"/ToUnicode ! 0 R").is_empty());
    assert!(budget.is_stopped());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 入力順・重複数に依らず整列結果と全再確保容量が独立モデルに一致する。
    #[test]
    fn reference_allocation_sorted_dedup_model(numbers in prop::collection::vec(0u32..100, 1..25), short in any::<bool>()) {
        let mut source = String::new();
        let mut total = 0u64;
        let mut capacity = 0usize;
        for (index, number) in numbers.iter().enumerate() {
            source.push_str(&format!("/ToUnicode {number} 0 R "));
            if index == capacity {
                capacity = (2 * capacity).max(4);
                total += (capacity * std::mem::size_of::<u32>()) as u64;
            }
        }
        let limit = total - u64::from(short);
        let budget = budget(limit);
        let actual = parse_tounicode_object_refs(&budget, source.as_bytes());
        if short {
            prop_assert!(actual.is_empty());
            exceeded(&budget, limit, total);
        } else {
            let mut expected = numbers;
            expected.sort_unstable();
            expected.dedup();
            prop_assert_eq!(actual, expected);
            prop_assert!(budget.check().is_ok());
            prop_assert!(budget.try_vector_capacity::<u8>(1).is_none());
            exceeded(&budget, limit, total + 1);
        }
    }
}
