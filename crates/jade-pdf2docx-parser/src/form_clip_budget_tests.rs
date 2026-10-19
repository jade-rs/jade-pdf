//! Clip頂点の全要求容量・再作成累積・停止保持を幾何結果とともに検査する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 幾何テストの単位行列。
const IDENTITY: [f32; 6] = [1., 0., 0., 1., 0., 0.];
/// 全ての辺で四頂点を保つページ矩形。
const PAGE: [f32; 4] = [0., 0., 100., 100.];
/// ページに含まれる矩形。
const INNER: [f32; 4] = [10., 10., 90., 90.];

/// metadata容量だけを指定する予算を作る。
fn budget(points: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: (points * std::mem::size_of::<Point>()) as u64,
        ..Default::default()
    })
}

/// 予算停止を型付きエラーの要求点数でも確認する。
fn assert_limit(target: &DecodeBudget, points: usize) {
    assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", actual, ..
    }) if actual == (points * std::mem::size_of::<Point>()) as u64));
}

/// 初期四頂点の境界を含み、領域未指定では配列を確保しない。
#[test]
fn page_rectangle_capacity_is_inclusive() {
    for points in [3, 4] {
        let target = budget(points);
        assert!(Clip::page(&target, None, None).unwrap().polygon.is_none());
        let clip = Clip::page(&target, Some(PAGE), None);
        assert_eq!(clip.is_some(), points == 4);
        if points == 3 {
            assert_limit(&target, 4);
        } else {
            assert_eq!(clip.unwrap().polygon.unwrap().len(), 4);
            target.check().unwrap();
        }
    }
}

/// ページとcropの頂点・四辺の各交差用容量を別々に累積する。
#[test]
fn page_crop_reserves_all_intermediate_arrays() {
    for points in [27, 28] {
        let target = budget(points);
        let clip = Clip::page(&target, Some(PAGE), Some(INNER));
        assert_eq!(clip.is_some(), points == 28);
        if points == 27 {
            assert_limit(&target, 28);
        } else {
            assert_eq!(
                clip.unwrap().bounds(ParsedBounds {
                    x0: 0.,
                    y0: 0.,
                    x1: 100.,
                    y1: 100.
                }),
                Some(ParsedBounds {
                    x0: 10.,
                    y0: 10.,
                    x1: 90.,
                    y1: 90.
                })
            );
            target.check().unwrap();
        }
    }
}

/// 親領域のない子は四頂点だけを保存する。
#[test]
fn child_without_parent_has_only_rectangle_capacity() {
    let parent = Clip::page(&budget(0), None, None).unwrap();
    for points in [3, 4] {
        let target = budget(points);
        let child = parent.child(&target, INNER, IDENTITY);
        assert_eq!(child.is_some(), points == 4);
        if points == 3 {
            assert_limit(&target, 4);
        } else {
            target.check().unwrap();
        }
    }
}

/// 子頂点と各辺の保存失敗でも親領域を変更せず、部分Clipを返さない。
#[test]
fn child_failure_at_each_stage_preserves_parent() {
    let parent = Clip::page(&budget(4), Some(PAGE), None).unwrap();
    let before = parent.polygon.clone();
    for (points, next) in [(3, 4), (4, 9), (9, 14), (14, 19), (19, 24)] {
        let target = budget(points);
        assert!(parent.child(&target, INNER, IDENTITY).is_none());
        assert_eq!(parent.polygon, before);
        assert_limit(&target, next);
    }
    let target = budget(24);
    assert!(parent.child(&target, INNER, IDENTITY).is_some());
    assert_eq!(parent.polygon, before);
    target.check().unwrap();
}

/// 回転で八頂点になった親も複製せず、各辺の九slotを確保前に累積する。
#[test]
fn octagonal_parent_requires_all_intersection_capacity() {
    let page = Clip::page(&budget(4), Some(PAGE), None).unwrap();
    let parent = page
        .child(&budget(100), PAGE, [0.7, 0.7, -0.7, 0.7, 50., -20.])
        .unwrap();
    assert_eq!(parent.polygon.as_ref().unwrap().len(), 8);
    for points in [39, 40] {
        let target = budget(points);
        let child = parent.child(&target, PAGE, IDENTITY);
        assert_eq!(child.is_some(), points == 40);
        if let Some(child) = child {
            assert_eq!(child.polygon, parent.polygon);
            target.check().unwrap();
        } else {
            assert_limit(&target, 40);
        }
    }
}

/// 文字矩形の交差も初回頂点と全交差配列の累積境界を守る。
#[test]
fn text_bounds_capacity_is_inclusive() {
    let parent = Clip::page(&budget(4), Some(PAGE), None).unwrap();
    for points in [23, 24] {
        let target = budget(points);
        let bounds = parent.transformed_bounds(&target, INNER, IDENTITY);
        assert_eq!(
            bounds,
            (points == 24).then_some(ParsedBounds {
                x0: 10.,
                y0: 10.,
                x1: 90.,
                y1: 90.
            })
        );
        if points == 23 {
            assert_limit(&target, 24);
        } else {
            target.check().unwrap();
        }
    }
}

/// 完成結果を解放しても次の文字交差を累積し、エラー消費後も再開しない。
#[test]
fn repeated_text_bounds_are_cumulative_and_stop_is_retained() {
    let parent = Clip::page(&budget(4), Some(PAGE), None).unwrap();
    for points in [47, 48] {
        let target = budget(points);
        assert!(
            parent
                .transformed_bounds(&target, INNER, IDENTITY)
                .is_some()
        );
        assert_eq!(
            parent
                .transformed_bounds(&target, INNER, IDENTITY)
                .is_some(),
            points == 48
        );
        if points == 47 {
            assert_limit(&target, 48);
            assert!(parent.child(&target, INNER, IDENTITY).is_none());
            assert!(Clip::page(&target, None, None).is_none());
            assert!(
                parent
                    .transformed_bounds(&target, INNER, IDENTITY)
                    .is_none()
            );
        } else {
            target.check().unwrap();
        }
    }
}

/// 退化・非有限・特異行列は可視頂点を保存しない。
#[test]
fn invisible_rectangles_require_no_storage() {
    let target = budget(0);
    for (rect, matrix) in [
        (PAGE, [0.; 6]),
        ([0., 0., 0., 1.], IDENTITY),
        ([0., 0., f32::NAN, 1.], IDENTITY),
    ] {
        assert!(rectangle(&target, rect, matrix).unwrap().is_empty());
    }
    target.check().unwrap();
}

/// 既に停止した予算では退化矩形や空交差でも完成結果を返さない。
#[test]
fn stopped_budget_rejects_empty_geometry() {
    let target = budget(0);
    assert!(rectangle(&target, PAGE, IDENTITY).is_none());
    assert_limit(&target, 4);
    assert!(rectangle(&target, [0.; 4], IDENTITY).is_none());
    assert!(intersect(&target, Cow::Borrowed(&[]), &[]).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// ページ/cropの生成列で、全配列要求容量・再作成・停止が独立モデルと一致する。
    #[test]
    fn page_sequence_matches_cumulative_capacity_model(
        operations in prop::collection::vec((any::<bool>(), any::<bool>()), 0..40),
        capacity in 0usize..400,
    ) {
        let target = budget(capacity);
        let mut spent = 0;
        let mut stopped = false;
        for (media, crop) in operations {
            let needed = match (media, crop) { (true, true) => 28, (true, false) | (false, true) => 4, (false, false) => 0 };
            if !stopped {
                if spent + needed > capacity { stopped = true; }
                else { spent += needed; }
            }
            let actual = Clip::page(&target, media.then_some(PAGE), crop.then_some(INNER));
            prop_assert_eq!(actual.is_some(), !stopped);
            if let Some(actual) = actual {
                prop_assert_eq!(actual.polygon.as_ref().map(Vec::len), (media || crop).then_some(4));
            }
        }
        prop_assert_eq!(target.check().is_err(), stopped);
    }
}
