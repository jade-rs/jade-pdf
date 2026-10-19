//! 未信頼キーmapの境界・更新履歴を独立モデルで検査する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// slot確保だけを制限する共有予算を作る。
fn budget(limit: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit as u64,
        ..Default::default()
    })
}

/// 停止したmapは既存キーの更新も禁止し、保留エラー消費後に再開しない。
#[test]
fn consumed_stop_rejects_existing_key_updates() {
    let target = budget(4 * std::mem::size_of::<Option<(u16, u16)>>());
    let mut map = Map::default();
    map.insert(&target, 1u16, 10u16).unwrap();
    map.insert(&target, 2, 20).unwrap();
    assert!(map.insert(&target, 3, 30).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    assert!(map.insert(&target, 1, 99).is_none());
    assert_eq!(map.get(&1), Some(&10));
    assert_eq!(map.get(&3), None);
}

proptest! {
    /// 重複・拡張・停止を含む履歴をBTreeMapと件数ベース容量モデルで比較する。
    #[test]
    fn updates_match_independent_model(history in prop::collection::vec((0u16..50, any::<u16>()), 0..120), limit in 0usize..5000) {
        let target = budget(limit);
        let mut map = Map::default();
        let mut expected = BTreeMap::new();
        let mut slots = 0usize;
        let mut charged = 0usize;
        let mut stopped = false;
        for (key, value) in history {
            if !stopped && !expected.contains_key(&key) && expected.len() >= slots / 2 {
                let next = (slots * 2).max(4);
                charged += next * std::mem::size_of::<Option<(u16, u16)>>();
                stopped = charged > limit;
                if !stopped { slots = next; }
            }
            prop_assert_eq!(map.insert(&target, key, value).is_some(), !stopped);
            if !stopped { expected.insert(key, value); }
            for key in 0..50 {
                prop_assert_eq!(map.get(&key), expected.get(&key));
            }
            prop_assert_eq!(map.is_empty(), expected.is_empty());
        }
        prop_assert_eq!(target.is_stopped(), stopped);
    }
}
