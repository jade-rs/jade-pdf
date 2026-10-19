//! 累積要求容量・失敗後の保存値・FIFO と集合の独立モデルを検証する。
use super::*;
use crate::ParseLimits;
use proptest::prelude::*;
use std::collections::{BTreeSet, VecDeque};

/// metadata だけの予算を指定する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 最初と次の全配列容量を加算し、失敗時に既存要素を移動しない。
#[test]
fn vector_growth_is_cumulative_and_inclusive() {
    let total = 12 * std::mem::size_of::<u32>() as u64;
    for limit in [total - 1, total] {
        let budget = budget(limit);
        let mut values = Vec::new();
        for value in 0..4u32 {
            push(&budget, &mut values, value).unwrap();
        }
        let result = push(&budget, &mut values, 4);
        if limit == total {
            result.unwrap();
            assert_eq!(values, [0, 1, 2, 3, 4]);
            values.pop();
            push(&budget, &mut values, 99).unwrap();
            assert_eq!(values, [0, 1, 2, 3, 99]);
        } else {
            assert!(
                matches!(result, Err(ParseError::ResourceLimit { actual, .. }) if actual == total)
            );
            assert_eq!(values, [0, 1, 2, 3]);
            values.pop();
            assert!(push(&budget, &mut values, 99).is_err());
            assert_eq!(values, [0, 1, 2]);
        }
    }
}

/// 重複と世代の区別、拡張時の累積境界と失敗後の不変性を固定する。
#[test]
fn set_growth_preserves_keys_and_generations() {
    let total = 12 * std::mem::size_of::<Option<(u32, u16)>>() as u64;
    for limit in [total - 1, total] {
        let budget = budget(limit);
        let mut set = Set::default();
        assert!(set.insert(&budget, (0, 0)).unwrap());
        assert!(set.insert(&budget, (0, u16::MAX)).unwrap());
        assert!(!set.insert(&budget, (0, 0)).unwrap());
        let result = set.insert(&budget, (u32::MAX, 0));
        if limit == total {
            assert!(result.unwrap());
            assert_eq!(set.len(), 3);
        } else {
            assert!(
                matches!(result, Err(ParseError::ResourceLimit { actual, .. }) if actual == total)
            );
            assert_eq!(set.len(), 2);
            assert!(!set.contains(&(u32::MAX, 0)));
            assert!(set.insert(&budget, (0, 0)).is_err());
        }
        assert!(set.contains(&(0, 0)));
        assert!(set.contains(&(0, u16::MAX)));
    }
}

/// wrap した ring の再確保でも FIFO を保ち、処理済み slot は再利用する。
#[test]
fn queue_reuses_slots_and_preserves_fifo_on_growth_failure() {
    let total = 12 * std::mem::size_of::<Option<u32>>() as u64;
    for limit in [total - 1, total] {
        let budget = budget(limit);
        let mut queue = Queue::default();
        for value in 0..4u32 {
            queue.push(&budget, value).unwrap();
        }
        for value in 0..3u32 {
            assert_eq!(queue.pop(), Some(value));
        }
        for value in 4..7u32 {
            queue.push(&budget, value).unwrap();
        }
        let result = queue.push(&budget, 7);
        if limit == total {
            result.unwrap();
        } else {
            assert!(
                matches!(result, Err(ParseError::ResourceLimit { actual, .. }) if actual == total)
            );
            assert!(queue.push(&budget, 99).is_err());
        }
        for value in 3..if limit == total { 8 } else { 7 } {
            assert_eq!(queue.pop(), Some(value));
        }
        assert_eq!(queue.pop(), None);
        if limit == total {
            queue.push(&budget, 99).unwrap();
            assert_eq!(queue.pop(), Some(99));
        }
    }
}

/// 配列値は本文を借用し、空配列は予算0、非空は要求容量で制限する。
#[test]
fn borrowed_arrays_are_bounded_and_validate_termination() {
    let input = b"[1 0 R << /A (x]y) >> [2 3] /Name]";
    let total = 4 * std::mem::size_of::<&[u8]>() as u64;
    assert_eq!(
        array_values(&budget(0), b"[]", 0).unwrap(),
        Some(Vec::new())
    );
    assert!(
        matches!(array_values(&budget(total - 1), input, 4), Err(ParseError::ResourceLimit { actual, .. }) if actual == total)
    );
    let values = array_values(&budget(total), input, 4).unwrap().unwrap();
    assert_eq!(
        values,
        [b"1 0 R".as_slice(), b"<< /A (x]y) >>", b"[2 3]", b"/Name"]
    );
    for value in values {
        assert!(value.as_ptr() >= input.as_ptr());
        assert!(value.as_ptr() as usize + value.len() <= input.as_ptr() as usize + input.len());
    }
    for input in [
        b"[1".as_slice(),
        b"[1] 2",
        b"[1 2 3 4 5]",
        b"[",
        b"1",
        b"[1 >>",
    ] {
        assert!(
            array_values(&budget(u64::MAX), input, 4).unwrap().is_none(),
            "{input:?}"
        );
    }
}

/// 確保エラーを消費した後も空配列や空集合の追加を再開させない。
#[test]
fn consumed_allocation_failure_keeps_workspace_stopped() {
    let budget = budget(u64::MAX);
    assert!(budget.metadata_vector::<u8>(usize::MAX).is_none());
    assert!(
        matches!(budget.check(), Err(ParseError::Io(error)) if error.kind() == std::io::ErrorKind::OutOfMemory)
    );
    assert!(push(&budget, &mut Vec::new(), 1u32).is_err());
    assert!(Set::default().insert(&budget, (1u32, 0u16)).is_err());
    assert!(Queue::default().push(&budget, 1u32).is_err());
    assert!(array_values(&budget, b"[]", 0).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意の push/pop 列を独立 FIFO と比べ、wrap・空列・拡張を混在させる。
    #[test]
    fn arbitrary_queue_operations_match_fifo(operations in prop::collection::vec((any::<bool>(), any::<u32>()), 0..256)) {
        let budget = budget(u64::MAX);
        let mut queue = Queue::default();
        let mut model = VecDeque::new();
        for (insert, value) in operations {
            if insert { queue.push(&budget, value).unwrap(); model.push_back(value); }
            else { prop_assert_eq!(queue.pop(), model.pop_front()); }
        }
        while let Some(value) = model.pop_front() { prop_assert_eq!(queue.pop(), Some(value)); }
        prop_assert_eq!(queue.pop(), None);
        prop_assert!(budget.check().is_ok());
    }

    /// 重複・異なる世代・任意の番号順序を独立集合と比較する。
    #[test]
    fn arbitrary_reference_keys_match_set(keys in prop::collection::vec((any::<u32>(), any::<u16>()), 0..128)) {
        let budget = budget(u64::MAX);
        let mut set = Set::default();
        let mut model = BTreeSet::new();
        for key in keys {
            prop_assert_eq!(set.insert(&budget, key).unwrap(), model.insert(key));
            prop_assert_eq!(set.len(), model.len());
            for key in &model { prop_assert!(set.contains(key)); }
            prop_assert!(!set.insert(&budget, key).unwrap());
        }
        prop_assert!(budget.check().is_ok());
    }
}
