//! Font幅表と共有本体を共通容量へ合算する境界を検証する。
use super::*;
use crate::{ParseError, ParseLimits, decode::DecodeBudget, font_metrics_cache::Cache};
use proptest::prelude::*;
use std::sync::Arc;

/// 用途別の上限を維持し、共通容量だけを変更する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_allocation_bytes: limit,
        ..Default::default()
    })
}

/// 最初の共通容量エラーを取得し、取得後も処理が再開しないことを確認する。
fn exceeded(budget: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "total allocation bytes", limit: found, actual: total,
    }) if found == limit && total == actual));
    assert!(budget.is_stopped());
    assert!(budget.try_vector_capacity::<u8>(0).is_none());
}

/// 配列がないFontでもArc本体と二つのcounterを予約する。
#[test]
fn owned_allocation_arc_body_is_inclusive_and_recreation_is_cumulative() {
    let bytes = (std::mem::size_of::<Metrics>() + 2 * std::mem::size_of::<usize>()) as u64;
    for limit in [bytes - 1, bytes] {
        let budget = budget(limit);
        let objects = Objects::new();
        let cache = Cache::new(&objects, &budget, &[]);
        let metrics = cache.read(b"<< /Subtype /Type1 >>");
        assert_eq!(metrics.is_some(), limit == bytes);
        if metrics.is_some() {
            drop(metrics);
            assert!(cache.read(b"<< /Subtype /Type1 >>").is_none());
            exceeded(&budget, limit, 2 * bytes);
        } else {
            exceeded(&budget, limit, bytes);
        }
        assert!(cache.read(b"<< /Subtype /Type1 >>").is_none());
    }
}

/// slotとArcを合算し、共有hitでは追加予約せず失敗値をcacheへ保存しない。
#[test]
fn owned_allocation_cache_hits_share_the_same_body() {
    let objects = Objects::from([(1, (0, b"<< /Subtype /Type1 >>".as_slice()))]);
    let probe = budget(0);
    assert!(Cache::new(&objects, &probe, &[]).get(1).is_none());
    let slot_bytes = match probe.check().unwrap_err() {
        ParseError::ResourceLimit { actual, .. } => actual,
        error => panic!("unexpected {error}"),
    };
    let total =
        slot_bytes + (std::mem::size_of::<Metrics>() + 2 * std::mem::size_of::<usize>()) as u64;
    for limit in [total - 1, total] {
        let budget = budget(limit);
        let mut cache = Cache::new(&objects, &budget, &[]);
        assert!(cache.get(99).is_none());
        let metrics = cache.get(1);
        assert_eq!(metrics.is_some(), limit == total);
        if let Some(metrics) = metrics {
            for _ in 0..64 {
                assert!(Arc::ptr_eq(&metrics, &cache.get(1).unwrap()));
            }
            assert!(budget.try_vector_capacity::<u8>(1).is_none());
            exceeded(&budget, limit, total + 1);
        } else {
            exceeded(&budget, limit, total);
        }
        assert!(cache.get(1).is_none());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 借用辞書から再作成したBoxと別用途のVecを独立byteモデルで合算する。
    #[test]
    fn owned_allocation_simple_width_boxes_share_the_limit(
        count in 1usize..8, prefix in 0usize..32, short in any::<bool>(),
    ) {
        let total = (prefix + count * 256 * std::mem::size_of::<f32>()) as u64;
        let limit = total - u64::from(short);
        let budget = budget(limit);
        drop(budget.metadata_vector::<u8>(prefix).unwrap());
        let objects = Objects::new();
        for index in 0..count {
            let metrics = Metrics::read(&objects,
                b"<< /Subtype /Type1 /FirstChar 65 /Widths [600] >>", &budget, &[]);
            prop_assert_eq!(metrics.widths.is_some(), !(short && index + 1 == count));
        }
        if short { exceeded(&budget, limit, total); }
        else {
            prop_assert!(budget.check().is_ok());
            prop_assert!(budget.try_vector_capacity::<u8>(1).is_none());
            exceeded(&budget, limit, total + 1);
        }
    }
}
