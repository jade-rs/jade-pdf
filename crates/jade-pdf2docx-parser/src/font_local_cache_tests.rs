//! Font局所cache外枠の予算外確保をpayloadと独立に再現する。
use super::*;
use crate::{ParseError, ParseLimits};
use std::cell::Cell;

/// metadata slotだけを制限し、Font payloadの予算と分離する。
fn budget(limit: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit as u64,
        ..Default::default()
    })
}

/// 空Font mapでもcache slotが必要で、拒否時にはfactoryを実行しない。
#[test]
fn zero_capacity_rejects_cache_before_build() {
    let target = budget(0);
    let builds = Cell::new(0);
    let mut cache = Cache::default();
    let fonts = cache.get_or_build(&target, b"<<>>", || {
        builds.set(builds.get() + 1);
        PageFontResources::default()
    });
    assert!(fonts.is_none());
    assert_eq!(builds.get(), 0);
    assert!(cache.entries.is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// cacheの解放後も新slotの要求容量を同じ予算へ累積する。
#[test]
fn recreation_requires_cumulative_cache_capacity() {
    let one = 4 * std::mem::size_of::<Option<(Key, PageFontResources)>>();
    let target = budget(2 * one - 1);
    let mut first = Cache::default();
    assert!(
        first
            .get_or_build(&target, b"<<>>", PageFontResources::default)
            .is_some()
    );
    drop(first);
    let mut second = Cache::default();
    assert!(
        second
            .get_or_build(&target, b"<<>>", PageFontResources::default)
            .is_none()
    );
    assert!(second.entries.is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// factory内で停止した部分mapを完成cacheとして保存しない。
#[test]
fn stopped_build_is_not_cached() {
    let one = 4 * std::mem::size_of::<Option<(Key, PageFontResources)>>();
    let target = budget(one);
    let builds = Cell::new(0);
    let mut cache = Cache::default();
    let fonts = cache.get_or_build(&target, b"<<>>", || {
        builds.set(builds.get() + 1);
        target.charge_metadata_workspace_bytes(one as u64 + 1);
        PageFontResources::default()
    });
    assert!(fonts.is_none());
    assert_eq!(builds.get(), 1);
    assert!(cache.entries.is_empty());
}

/// 保存するFont map本体を含むslotの初回要求容量を独立に計算する。
fn first_capacity() -> usize {
    4 * std::mem::size_of::<Option<(Key, PageFontResources)>>()
}

/// inclusiveな初回境界と共有hitを検査し、hitではfactoryも再予約も行わない。
#[test]
fn exact_capacity_and_hits_do_not_rebuild() {
    let one = first_capacity();
    for limit in [one - 1, one] {
        let target = budget(limit);
        let mut cache = Cache::default();
        let builds = Cell::new(0);
        for _ in 0..64 {
            let fonts = cache.get_or_build(&target, b"<<>>", || {
                builds.set(builds.get() + 1);
                PageFontResources::default()
            });
            assert_eq!(fonts.is_some(), limit == one);
        }
        assert_eq!(builds.get(), usize::from(limit == one));
        if limit == one {
            target.check().unwrap();
        } else {
            assert!(
                matches!(target.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == one as u64)
            );
        }
    }
}

/// 拡張の新配列全体を予約し、失敗後は以前の完成hitも公開しない。
#[test]
fn denied_growth_blocks_completed_hits_after_consumed_error() {
    let one = first_capacity();
    let target = budget(one);
    let mut cache = Cache::default();
    let sources = [b"one".as_slice(), b"two".as_slice(), b"three".as_slice()];
    let builds = Cell::new(0);
    for (index, source) in sources.iter().enumerate() {
        assert_eq!(
            cache
                .get_or_build(&target, source, || {
                    builds.set(builds.get() + 1);
                    PageFontResources::default()
                })
                .is_some(),
            index < 2
        );
    }
    assert_eq!(builds.get(), 2);
    assert!(
        matches!(target.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == (3 * one) as u64)
    );
    assert!(
        cache
            .get_or_build(&target, sources[0], || panic!("stopped factory"))
            .is_none()
    );
}

/// 内容が同じ別入力と、同じ位置で長さの違う辞書を取り違えない。
#[test]
fn identity_uses_both_pointer_and_length() {
    let first = b"<<>>".to_vec();
    let second = first.clone();
    assert_ne!(first.as_ptr(), second.as_ptr());
    let target = budget(3 * first_capacity());
    let mut cache = Cache::default();
    let builds = Cell::new(0);
    for source in [
        &first[..],
        &second[..],
        &first[..2],
        &first[..],
        &second[..],
        &first[..2],
    ] {
        assert!(
            cache
                .get_or_build(&target, source, || {
                    builds.set(builds.get() + 1);
                    PageFontResources::default()
                })
                .is_some()
        );
    }
    assert_eq!(builds.get(), 3);
    target.check().unwrap();
}

/// 異なるcacheは同じ入力でも完成mapを共有せず、正確な再作成容量を使う。
#[test]
fn exact_recreation_capacity_is_inclusive() {
    let target = budget(2 * first_capacity());
    let builds = Cell::new(0);
    for _ in 0..2 {
        let mut cache = Cache::default();
        assert!(
            cache
                .get_or_build(&target, b"<<>>", || {
                    builds.set(builds.get() + 1);
                    PageFontResources::default()
                })
                .is_some()
        );
    }
    assert_eq!(builds.get(), 2);
    target.check().unwrap();
}

/// factoryの局所Font entryが完成した後の容量超過でも、その部分mapを保存しない。
#[test]
fn partial_font_payload_is_discarded_before_cache_insertion() {
    let inner = 4 * std::mem::size_of::<Option<(String, String)>>() + 2;
    let target = budget(first_capacity() + inner);
    let mut cache = Cache::default();
    let builds = Cell::new(0);
    assert!(
        cache
            .get_or_build(&target, b"<<>>", || {
                builds.set(builds.get() + 1);
                let mut fonts = PageFontResources::default();
                let value = crate::metadata_map::copy(&target, "A").unwrap();
                crate::metadata_map::save(&target, &mut fonts.base_fonts, "F", value).unwrap();
                assert!(fonts.base_fonts.get("F").is_some());
                assert!(target.charge_metadata_workspace_bytes(1).is_none());
                fonts
            })
            .is_none()
    );
    assert_eq!(builds.get(), 1);
    assert!(cache.entries.is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    assert!(
        cache
            .get_or_build(&target, b"<<>>", || panic!("consumed stop"))
            .is_none()
    );
}

use proptest::prelude::*;
proptest! {
    /// alias履歴を独立した件数/容量モデルと比較し、拒否後のfactory副作用も禁止する。
    #[test]
    fn alias_history_matches_capacity_and_factory_model(history in prop::collection::vec(0usize..20, 0..100), limit in 0usize..25000) {
        let sources: Vec<_> = (0..20).map(|i| format!("resource{i}").into_bytes()).collect();
        let target = budget(limit);
        let mut cache = Cache::default();
        let mut seen = [false; 20];
        let mut count = 0usize;
        let mut slots = 0usize;
        let mut charged = 0usize;
        let mut stopped = false;
        let builds = Cell::new(0usize);
        for index in history {
            if !stopped && !seen[index] {
                if count >= slots / 2 {
                    let next = (slots * 2).max(4);
                    charged += next * std::mem::size_of::<Option<(Key, PageFontResources)>>();
                    stopped = charged > limit;
                    if !stopped { slots = next; }
                }
                if !stopped { seen[index] = true; count += 1; }
            }
            let fonts = cache.get_or_build(&target, &sources[index], || {
                builds.set(builds.get() + 1);
                PageFontResources::default()
            });
            prop_assert_eq!(fonts.is_some(), !stopped);
            prop_assert_eq!(builds.get(), count);
            prop_assert_eq!(cache.entries.is_empty(), count == 0);
        }
        prop_assert_eq!(target.is_stopped(), stopped);
        if stopped {
            target.check().unwrap_err();
            for source in &sources {
                prop_assert!(cache.get_or_build(&target, source, || panic!("consumed stop")).is_none());
            }
        } else { target.check().unwrap(); }
    }

    /// 別cacheの作成/解放も返金せず、回数とnative slot容量の積で境界を判定する。
    #[test]
    fn recreation_matches_non_refunding_model(count in 1usize..20, short in any::<bool>()) {
        let limit = count * first_capacity() - usize::from(short);
        let target = budget(limit);
        let builds = Cell::new(0);
        for index in 0..count {
            let mut cache = Cache::default();
            let allowed = !short || index + 1 < count;
            prop_assert_eq!(cache.get_or_build(&target, b"<<>>", || {
                builds.set(builds.get() + 1);
                PageFontResources::default()
            }).is_some(), allowed);
        }
        prop_assert_eq!(builds.get(), count - usize::from(short));
        prop_assert_eq!(target.is_stopped(), short);
    }
}
