//! 借用resource索引の名前互換性・重複選択・全候補容量を検査する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 索引slot数だけを許可する予算を作る。
fn budget(slots: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: (slots * std::mem::size_of::<usize>()) as u64,
        ..Default::default()
    })
}

/// 重複候補も数え、容量不足では部分索引を公開しない。
#[test]
fn index_capacity_counts_duplicate_candidates() {
    for slots in [2, 3] {
        let target = budget(slots);
        let refs = References::new(
            &target,
            vec![(0, "F".into(), 9), (0, "F".into(), 3), (1, "F".into(), 7)],
        );
        assert_eq!(refs.is_some(), slots == 3);
        if let Some(refs) = refs {
            assert_eq!(refs.get(0, b"#46"), Some(3));
            assert_eq!(refs.get(1, b"F"), Some(7));
            assert_eq!(refs.get(2, b"F"), None);
            target.check().unwrap();
        } else {
            assert!(
                matches!(target.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == (3 * std::mem::size_of::<usize>()) as u64)
            );
        }
    }
}

/// byte対応の名前・literal hash・不正escape・prefixを保存用heapなしで照合する。
#[test]
fn binary_and_escaped_keys_keep_existing_semantics() {
    let target = budget(3);
    let refs = References::new(
        &target,
        vec![(0, "ÿ".into(), 2), (0, "#".into(), 3), (0, "Fm".into(), 4)],
    )
    .unwrap();
    for (raw, expected) in [
        (b"#FF".as_slice(), Some(2)),
        (b"\xff", Some(2)),
        (b"#23", Some(3)),
        (b"F#6d", Some(4)),
        (b"F", None),
        (b"FmX", None),
        (b"Fm#", None),
        (b"\xc3\xbf", None),
    ] {
        assert_eq!(refs.get(0, raw), expected);
    }
    target.check().unwrap();
}

/// 解放後の索引保存も累積し、停止エラー消費後は空索引でも再開しない。
#[test]
fn recreation_is_cumulative_and_stop_is_retained() {
    let target = budget(1);
    drop(References::new(&target, vec![(0, "F".into(), 1)]).unwrap());
    assert!(References::new(&target, vec![(0, "F".into(), 1)]).is_none());
    assert!(target.check().is_err());
    assert!(References::new(&target, Vec::new()).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 整列索引のlookupが、従来のbyte復号キーと最後の候補を選ぶ独立モデルに一致する。
    #[test]
    fn lookup_matches_last_decoded_candidate(
        entries in prop::collection::vec((0usize..5, prop::collection::vec(any::<u8>(), 0..20), any::<u32>()), 0..40),
        page in 0usize..6,
        raw in prop::collection::vec(any::<u8>(), 0..20)
    ) {
        let values = entries.iter().filter_map(|(page, raw, number)| Some((*page, crate::metadata::resource_name(raw)?, *number))).collect::<Vec<_>>();
        let expected = crate::metadata::resource_name(&raw).and_then(|name| values.iter().rev().find(|(candidate, key, _)| *candidate == page && *key == name).map(|(_, _, number)| *number));
        let refs = References::new(&budget(values.len()), values).unwrap();
        prop_assert_eq!(refs.get(page, &raw), expected);
        for (page, raw, _) in &entries {
            if let Some(name) = crate::metadata::resource_name(raw) {
                let expected = entries.iter().rev().find(|(candidate, key, _)| *candidate == *page && crate::metadata::resource_name(key).as_ref() == Some(&name)).map(|(_, _, number)| *number);
                prop_assert_eq!(refs.get(*page, raw), expected);
                let escaped = raw.iter().map(|byte| format!("#{byte:02X}")).collect::<String>();
                let literal = raw.iter().map(|byte| char::from(*byte)).collect::<String>();
                let expected = entries.iter().rev().find(|(candidate, key, _)| *candidate == *page && crate::metadata::resource_name(key).as_ref() == Some(&literal)).map(|(_, _, number)| *number);
                prop_assert_eq!(refs.get(*page, escaped.as_bytes()), expected);
            }
        }
    }
}
