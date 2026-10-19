//! ページ状態とq保存容量の独立モデル、再作成累積と座標不変条件。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;
use std::mem::size_of;

/// metadataだけを制限する予算を作る。
fn budget(capacity: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity as u64,
        ..Default::default()
    })
}

/// 初回4slotと倍増時の全要求slotを独立に求める。
fn slots(count: usize) -> usize {
    let mut total = 0;
    let mut capacity = 0;
    while capacity < count {
        capacity = (capacity * 2).max(4);
        total += capacity;
    }
    total
}

/// ページ状態の初回/拡張はinclusiveで、再訪時は同じ状態を使う。
#[test]
fn page_state_capacity_is_inclusive_and_reused() {
    for count in [1, 4, 5, 9] {
        let mut contents = (0..count)
            .rev()
            .map(|page| (page, b"2 0 0 3 4 5 cm".to_vec()))
            .collect::<Vec<_>>();
        contents.extend((0..count).map(|page| (page, b"/Im Do".to_vec())));
        let cost = slots(count) * size_of::<(usize, State<'_>)>();
        for capacity in [cost - 1, cost] {
            let target = budget(capacity);
            let mut painted = 0;
            let result = visit(&target, &contents, |_, _, matrix| {
                assert_eq!(matrix, [2., 0., 0., 3., 4., 5.]);
                painted += 1;
                Some(())
            });
            assert_eq!(result.is_some(), capacity == cost);
            assert_eq!(painted, if capacity == cost { count } else { 0 });
            assert_eq!(target.check().is_ok(), capacity == cost);
        }
    }
}

/// q保存列のpop後は容量を再利用し、解放後の再作成だけを累積する。
#[test]
fn graphics_stack_capacity_and_recreation_are_cumulative() {
    for depth in [1, 4, 5, 9] {
        let content = format!("{}{} q Q /Im Do", "q ".repeat(depth), "Q ".repeat(depth));
        let contents = [(0, content.into_bytes())];
        let cost = 4 * size_of::<(usize, State<'_>)>() + slots(depth) * size_of::<GraphicsState>();
        for capacity in [cost - 1, cost, 2 * cost - 1, 2 * cost] {
            let target = budget(capacity);
            for pass in 1..=2 {
                let mut painted = 0;
                let result = visit(&target, &contents, |_, _, _| {
                    painted += 1;
                    Some(())
                });
                assert_eq!(result.is_some(), pass * cost <= capacity);
                assert_eq!(painted, usize::from(pass * cost <= capacity));
            }
            assert_eq!(target.check().is_ok(), 2 * cost <= capacity);
            if target.is_stopped() {
                assert!(visit(&target, &contents, |_, _, _| panic!("stopped draw")).is_none());
                assert!(visit(&target, &[], |_, _, _| panic!("stopped empty draw")).is_none());
            }
        }
    }
}

/// 保存深度超過は容量不足より先に通知し、消費後も停止を保持する。
#[test]
fn graphics_depth_error_remains_first() {
    let target = DecodeBudget::new(&ParseLimits {
        max_container_nesting: 1,
        ..Default::default()
    });
    assert!(
        visit(&target, &[(0, b"q q /Im Do".to_vec())], |_, _, _| panic!(
            "over-depth draw"
        ))
        .is_none()
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "Form graphics state depth",
            limit: 1,
            actual: 2
        })
    ));
    assert!(
        visit(&target, &[(0, b"/Im Do".to_vec())], |_, _, _| panic!(
            "stopped draw"
        ))
        .is_none()
    );
}

/// 復号名のheapを作らず長いnameを借用し、コメントで分断したDoを保持する。
#[test]
fn borrowed_name_requires_only_page_state_slots() {
    let mut content = b"/".to_vec();
    content.extend(std::iter::repeat_n(b'A', 100_000));
    content.extend_from_slice(b"% comment\nDo");
    let contents = [(0, content)];
    let target = budget(4 * size_of::<(usize, State<'_>)>());
    let mut painted = 0;
    visit(&target, &contents, |_, name, _| {
        assert_eq!(name.len(), 100_000);
        assert_eq!(name.as_ptr(), contents[0].1[1..].as_ptr());
        painted += 1;
        Some(())
    })
    .unwrap();
    assert_eq!(painted, 1);
    target.check().unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 独立なscale/translateモデルとq復元はContents境界に依存しない。
    #[test]
    fn matrix_composition_matches_independent_model(a in 1i16..20, d in 1i16..20, e in -20i16..20, f in -20i16..20, u in 1i16..20, v in 1i16..20, x in -20i16..20, y in -20i16..20) {
        let contents = [
            (0, format!("{a} 0 0 {d} {e} {f} cm q {u} 0 0 {v} {x} {y} cm /Im").into_bytes()),
            (1, b"/Other Do".to_vec()),
            (0, b"Do Q /Im Do".to_vec()),
        ];
        let target = budget(usize::MAX);
        let mut actual = Vec::new();
        visit(&target, &contents, |page, _, matrix| { actual.push((page, matrix)); Some(()) }).unwrap();
        prop_assert_eq!(actual, [
            (1, [1., 0., 0., 1., 0., 0.]),
            (0, [(a*u) as f32, 0., 0., (d*v) as f32, (a*x+e) as f32, (d*y+f) as f32]),
            (0, [a as f32, 0., 0., d as f32, e as f32, f as f32]),
        ]);
        prop_assert!(target.check().is_ok());
    }
}

/// Contents境界をまたぐ色・保存stackをページごとに分離し、stroke色を無視する。
#[test]
fn fill_color_is_scoped_by_page_and_saved_across_contents() {
    let target = DecodeBudget::new(&ParseLimits::default());
    let contents = [
        (0, b"1 0 0 rg q .5 g".to_vec()),
        (1, b"/Im Do".to_vec()),
        (0, b"/Im Do Q 0 1 0 RG /Im Do".to_vec()),
    ];
    let mut draws = Vec::new();
    visit_painted(&target, &contents, |page, _, _, color| {
        draws.push((page, color.color));
        Some(())
    })
    .unwrap();
    assert_eq!(
        draws,
        [
            (1, Some([0; 3])),
            (0, Some([128; 3])),
            (0, Some([255, 0, 0]))
        ]
    );
}
