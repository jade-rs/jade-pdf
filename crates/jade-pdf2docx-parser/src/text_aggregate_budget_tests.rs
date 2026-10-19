//! ページ/Form文字結果の集約slotの累積容量を固定する。
use super::*;
use crate::{ParseError, ParseLimits, ParsedBounds, ParsedTextElement};
use proptest::prelude::*;

/// 文字結果集約だけに適用する容量を指定する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 属性や文字列の保存は対象予算から分離して結果要素を用意する。
fn element(text: &str) -> ParsedTextElement {
    ParsedTextElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 0.,
            y0: 0.,
            x1: 10.,
            y1: 10.,
        },
        font_size: 10.,
        font_name: None,
        fill_color: None,
        text: text.into(),
    }
}

/// 既存結果への初回拡張は全slotを予約し、不足時は既存結果を変更しない。
#[test]
fn aggregate_initial_capacity_is_inclusive() {
    let cost = 2 * std::mem::size_of::<ParsedTextElement>() as u64;
    for limit in [cost - 1, cost] {
        let target = budget(limit);
        let mut result = vec![element("first")];
        let added = vec![element("second")];
        assert_eq!(append(&target, &mut result, added).is_some(), limit == cost);
        if limit == cost {
            assert_eq!(
                result.iter().map(|v| v.text.as_str()).collect::<Vec<_>>(),
                ["first", "second"]
            );
            assert!(target.check().is_ok());
        } else {
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].text, "first");
            assert!(
                matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost)
            );
        }
    }
}

/// 再確保の新slot全体を累積し、既存容量は使い回して文字列を複製しない。
#[test]
fn aggregate_growth_counts_full_capacity_and_moves_text() {
    let slot = std::mem::size_of::<ParsedTextElement>() as u64;
    for short in [false, true] {
        let target = budget(4 * slot - u64::from(short));
        let first = element("first");
        let pointer = first.text.as_ptr();
        let mut result = vec![first, element("second")];
        let expanded = append(&target, &mut result, vec![element("third")]);
        assert_eq!(expanded.is_none(), short);
        assert_eq!(result[0].text.as_ptr(), pointer);
        if short {
            assert_eq!(result.len(), 2);
            assert!(target.check().is_err());
        } else {
            append(&target, &mut result, vec![element("fourth")]).unwrap();
            assert_eq!(result.len(), 4);
            assert!(target.check().is_ok());
            drop(result);
            let mut recreated = vec![element("again")];
            assert!(append(&target, &mut recreated, vec![element("later")]).is_none());
            assert!(target.check().is_err());
        }
        assert!(append(&target, &mut Vec::new(), Vec::new()).is_none());
        assert!(target.is_stopped());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 連続した集約の全容量予約と結果順は倍増モデルに一致する。
    #[test]
    fn aggregate_capacity_matches_model(counts in prop::collection::vec(1usize..5, 2..6), short in any::<bool>()) {
        let (mut length, mut capacity, mut slots) = (0usize, 0usize, 0usize);
        for count in &counts {
            length += count;
            if capacity == 0 {
                capacity = *count;
            } else if length > capacity {
                capacity = length.max(capacity * 2);
                slots += capacity;
            }
        }
        let target = budget((slots * std::mem::size_of::<ParsedTextElement>()) as u64 - u64::from(short));
        let mut result = Vec::new();
        let mut added = 0usize;
        for count in counts {
            let addition = (added..added + count).map(|index| element(&index.to_string())).collect();
            if append(&target, &mut result, addition).is_none() { break; }
            added += count;
        }
        prop_assert_eq!(target.is_stopped(), short);
        prop_assert_eq!(target.check().is_err(), short);
        if !short { prop_assert_eq!(result.len(), length); }
        for (index, value) in result.iter().enumerate() { prop_assert_eq!(&value.text, &index.to_string()); }
    }
}

/// 空の集約先は予約済みの本文結果配列を引き継ぎ、slotも文字列も再確保しない。
#[test]
fn empty_aggregate_reuses_owned_storage() {
    let target = budget(0);
    let addition = vec![element("first"), element("second")];
    let pointer = addition.as_ptr();
    let mut result = Vec::new();
    append(&target, &mut result, addition).unwrap();
    assert_eq!(result.as_ptr(), pointer);
    assert_eq!(result.len(), 2);
    assert!(target.check().is_ok());
}
