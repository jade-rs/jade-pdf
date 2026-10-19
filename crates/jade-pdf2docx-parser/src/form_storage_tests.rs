//! ページ本文保存とForm cacheの容量・順序・共有・停止を検査する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

/// metadata保存容量だけを調整する予算を作る。
fn budget(capacity: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity as u64,
        ..Default::default()
    })
}

/// RcのVec本体と二つのcounterの要求容量を独立に計算する。
fn shared_capacity() -> usize {
    std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>()
}

/// index・ページslot・改行を含む本文・共有本体の容量を独立モデルで数える。
fn page_capacity(streams: &[(usize, Vec<u8>)]) -> usize {
    let count = streams
        .iter()
        .map(|stream| stream.0)
        .collect::<BTreeSet<_>>()
        .len();
    streams.len() * std::mem::size_of::<usize>()
        + count * (std::mem::size_of::<(usize, Rc<Vec<u8>>)>() + shared_capacity())
        + streams
            .iter()
            .map(|stream| stream.1.len() + 1)
            .sum::<usize>()
}

/// 占有率半分の4/8/16/...slotを独立モデルで累積する。
fn cache_capacity(count: usize) -> usize {
    [4, 8, 16, 32, 64, 128]
        .into_iter()
        .filter(|&capacity| count > capacity / 4 || capacity == 4 && count > 0)
        .map(|capacity| capacity * std::mem::size_of::<Option<(u32, Option<Rc<Vec<u8>>>)>>())
        .sum()
}

/// 保存超過の型と停止状態を確認し、エラー消費後の再開も禁止する。
fn assert_limit(target: &DecodeBudget) {
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    assert!(target.is_stopped());
}

/// page順と同page内のstream順を保持し、本文の連結境界に改行を入れる。
#[test]
fn page_order_and_inclusive_recreation_capacity() {
    let streams = vec![
        (2, b"z".to_vec()),
        (0, b"a".to_vec()),
        (2, b"b".to_vec()),
        (0, Vec::new()),
        (1, b"k".to_vec()),
    ];
    let one = page_capacity(&streams);
    for (limit, builds) in [(one - 1, 1), (one, 1), (2 * one - 1, 2), (2 * one, 2)] {
        let target = budget(limit);
        for pass in 0..builds {
            let output = pages(&target, streams.clone());
            if (pass + 1) * one <= limit {
                let output = output.unwrap();
                assert_eq!(
                    output
                        .iter()
                        .map(|(page, bytes)| (*page, bytes.as_slice()))
                        .collect::<Vec<_>>(),
                    [(0, b"a\n\n".as_slice()), (1, b"k\n"), (2, b"z\nb\n")]
                );
            } else {
                assert!(
                    output.is_none(),
                    "late-page failure must discard completed pages"
                );
            }
        }
        assert_eq!(target.check().is_ok(), builds * one <= limit);
    }
}

/// 空入力は無確保で成功するが、停止後は空入力でも処理を再開しない。
#[test]
fn empty_pages_do_not_resume_after_error_consumption() {
    let target = budget(0);
    assert!(pages(&target, Vec::new()).unwrap().is_empty());
    assert!(pages(&target, vec![(0, vec![])]).is_none());
    assert_limit(&target);
    assert!(pages(&target, Vec::new()).is_none());
}

/// cache hitは本文と共有本体を複製せず、欠落も再取得しない。
#[test]
fn payload_hit_and_missing_hit_require_no_new_capacity() {
    let target = budget(cache_capacity(2) + shared_capacity());
    let mut cache = Payloads::default();
    let first = cache
        .get_or_load(&target, 1, || Some(b"FORM".to_vec()))
        .unwrap();
    let second = cache
        .get_or_load(&target, 1, || panic!("shared hit must not reload"))
        .unwrap();
    assert!(Rc::ptr_eq(&first, &second));
    assert!(cache.get_or_load(&target, 2, || None).is_none());
    assert!(
        cache
            .get_or_load(&target, 2, || panic!("missing hit must not reload"))
            .is_none()
    );
    target.check().unwrap();
}

/// 初回slotや拡張slot不足では本文factoryを呼ばず、停止後のhitも拒否する。
#[test]
fn cache_growth_is_reserved_before_loading() {
    let target = budget(cache_capacity(2));
    let mut cache = Payloads::default();
    assert!(cache.get_or_load(&target, 1, || None).is_none());
    assert!(cache.get_or_load(&target, 2, || None).is_none());
    assert!(
        cache
            .get_or_load(&target, 3, || panic!("growth must precede loading"))
            .is_none()
    );
    assert_limit(&target);
    assert!(
        cache
            .get_or_load(&target, 1, || panic!("stopped hit must not reload"))
            .is_none()
    );
    let target = budget(0);
    assert!(
        Payloads::default()
            .get_or_load(&target, 1, || panic!("initial slots must precede loading"))
            .is_none()
    );
    assert_limit(&target);
}

/// 共有本体の容量が不足した場合、取得済みpayloadをcacheへ保存しない。
#[test]
fn shared_body_failure_does_not_cache_partial_payload() {
    let target = budget(cache_capacity(1) + shared_capacity() - 1);
    let mut cache = Payloads::default();
    assert!(cache.get_or_load(&target, 1, || Some(vec![1])).is_none());
    assert!(cache.entries.get(&1).is_none());
    assert_limit(&target);
}

/// 本文取得が別の予算で停止した場合、欠落としてcacheへ保存しない。
#[test]
fn payload_decode_failure_does_not_cache_missing_result() {
    let target = DecodeBudget::new(&ParseLimits {
        max_decoded_stream_bytes: 0,
        ..Default::default()
    });
    let mut cache = Payloads::default();
    assert!(
        cache
            .get_or_load(&target, 1, || target.copy_stream_bytes(b"a"))
            .is_none()
    );
    assert!(cache.entries.get(&1).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "decoded stream bytes",
            ..
        })
    ));
    assert!(
        cache
            .get_or_load(&target, 1, || panic!("stopped cache must not reload"))
            .is_none()
    );
}

/// cacheの解放後に再作成するslotと共有本体も累積へ数える。
#[test]
fn payload_cache_recreation_is_cumulative() {
    let one = cache_capacity(1) + shared_capacity();
    for capacity in [2 * one - 1, 2 * one] {
        let target = budget(capacity);
        for pass in 0..2 {
            let output = Payloads::default().get_or_load(&target, 1, || Some(Vec::new()));
            assert_eq!(output.is_some(), (pass + 1) * one <= capacity);
        }
        assert_eq!(target.check().is_ok(), capacity == 2 * one);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 入力順の独立mapモデルでinterleave・空本文・容量不足・再作成累積を検査する。
    #[test]
    fn page_join_matches_order_and_capacity_model(
        streams in prop::collection::vec((0usize..10, prop::collection::vec(any::<u8>(), 0..32)), 0..40),
        builds in 1usize..4,
        short in any::<bool>(),
    ) {
        let mut expected = BTreeMap::<usize, Vec<u8>>::new();
        for (page, content) in &streams {
            let joined = expected.entry(*page).or_default();
            joined.extend_from_slice(content);
            joined.push(b'\n');
        }
        let one = page_capacity(&streams);
        let capacity = (one * builds).saturating_sub(usize::from(short));
        let target = budget(capacity);
        for pass in 0..builds {
            let output = pages(&target, streams.clone());
            if (pass + 1) * one <= capacity {
                let actual = output.unwrap().into_iter().map(|(page, content)| (page, (*content).clone())).collect::<BTreeMap<_, _>>();
                prop_assert_eq!(&actual, &expected);
            } else {
                prop_assert!(output.is_none());
            }
        }
        prop_assert_eq!(target.check().is_ok(), one * builds <= capacity);
    }

    /// 任意のalias列で初出だけ取得し、共有hitとslot拡張の正確な境界を固定する。
    #[test]
    fn payload_cache_matches_first_load_capacity_model(
        numbers in prop::collection::vec(0u32..64, 0..72),
        short in any::<bool>(),
    ) {
        let unique = numbers.iter().copied().collect::<BTreeSet<_>>().len();
        let one = cache_capacity(unique) + unique * shared_capacity();
        let capacity = one.saturating_sub(usize::from(short));
        let target = budget(capacity);
        let mut cache = Payloads::default();
        let mut loaded = BTreeSet::new();
        let mut previous = BTreeMap::<u32, Rc<Vec<u8>>>::new();
        for number in numbers {
            let output = cache.get_or_load(&target, number, || {
                assert!(loaded.insert(number));
                Some(number.to_be_bytes().to_vec())
            });
            if let Some(bytes) = output {
                prop_assert_eq!(bytes.as_slice(), number.to_be_bytes());
                if let Some(old) = previous.get(&number) {
                    prop_assert!(Rc::ptr_eq(old, &bytes));
                }
                previous.insert(number, bytes);
            } else {
                prop_assert!(target.is_stopped());
            }
        }
        prop_assert_eq!(target.check().is_ok(), one <= capacity);
        if one <= capacity {
            prop_assert_eq!(loaded.len(), unique);
        }
    }
}
