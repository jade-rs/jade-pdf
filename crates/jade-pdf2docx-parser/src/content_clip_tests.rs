//! clip走査の容量停止とpath検証の境界。
use super::*;

/// 文字列・名前・container・comment内のWではclip用の退避容量を消費しない。
#[test]
fn fake_clipping_operators_require_no_clip_workspace() {
    for content in [
        b"q (W W*) Tj Q".as_slice(),
        b"q /W 1 Tf Q",
        b"q [W W*] pop Q",
        b"q << /operator (W) >> pop Q",
        b"q % W\nQ",
    ] {
        let budget = DecodeBudget::new(&crate::ParseLimits {
            max_total_metadata_workspace_bytes: 0,
            ..Default::default()
        });
        assert!(
            scan(&budget, content, [1., 0., 0., 1., 0., 0.])
                .entries
                .is_empty()
        );
        budget.check().unwrap();
    }
}

/// 64頂点の閉路は、末尾の始点重複を追加頂点として数えない。
#[test]
fn closed_polygon_accepts_exactly_sixty_four_vertices() {
    let budget = DecodeBudget::default();
    let mut path = Path::default();
    for index in 0..=64 {
        let angle = (index % 64) as f64 * std::f64::consts::TAU / 64.;
        path.push(&budget, [angle.cos(), angle.sin()]).unwrap();
    }
    assert_eq!(path.finish().unwrap().len(), 64);
}

/// 途中で予算が尽きると全timelineを破棄し、部分結果を可視性保証に使わない。
#[test]
fn allocation_failure_discards_completed_clip_entries() {
    let content = b"q 0 0 10 10 re W n Q q 1 1 8 8 re W n Q";
    let mut successes = 0;
    let mut failures = 0;
    for capacity in (0..2000).step_by(16) {
        let budget = DecodeBudget::new(&crate::ParseLimits {
            max_total_metadata_workspace_bytes: capacity,
            ..Default::default()
        });
        let clips = scan(&budget, content, [1., 0., 0., 1., 0., 0.]);
        if budget.check().is_err() {
            failures += 1;
            assert!(clips.entries.is_empty());
            assert!(
                scan(&budget, content, [1., 0., 0., 1., 0., 0.])
                    .entries
                    .is_empty()
            );
        } else {
            successes += 1;
            assert_eq!(clips.entries.len(), 4);
            assert!(clips.get(content.len()).is_none());
        }
    }
    assert!(successes > 0 && failures > 0);
}

/// 凸性検証は頂点上限・非有限・自己交差を非対応にし、向きを正規化する。
#[test]
fn invalid_paths_are_rejected_without_growing_past_the_vertex_cap() {
    let budget = DecodeBudget::default();
    for points in [
        vec![[0., 0.], [4., 0.], [1., 4.], [2., -1.], [3., 4.]],
        vec![[0., 0.], [f64::NAN, 1.], [1., 0.]],
        (0..65).map(|index| [index as f64, 0.]).collect(),
    ] {
        let mut path = Path::default();
        for point in points {
            path.push(&budget, point).unwrap();
        }
        assert!(path.points.len() <= 64);
        assert!(path.finish().is_none());
    }
    let mut path = Path::default();
    for point in [[0., 0.], [0., 4.], [4., 4.], [4., 0.]] {
        path.push(&budget, point).unwrap();
    }
    let polygon = path.finish().unwrap();
    assert!(side(polygon[0], polygon[1], polygon[2]) > 0.);
}
