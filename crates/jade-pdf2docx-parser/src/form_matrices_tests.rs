//! BT対応とgraphics state保存の容量境界・再利用・独立座標モデルを確認する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 初回4slotから倍増する保存配列の全要求容量を独立に数える。
fn slots(count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let mut capacity = 4;
    let mut total = 0;
    loop {
        total += capacity;
        if count <= capacity {
            return total;
        }
        capacity *= 2;
    }
}

/// metadata容量だけを制限する予算を作る。
fn budget(capacity: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity as u64,
        ..Default::default()
    })
}

/// BT位置はbyte順で保存され、q/Qと偽命令・inline image境界を保持する。
#[test]
fn positions_restore_matrix_and_ignore_fake_operators() {
    let content =
        b"(BT q) /BT [BT] << /Key (BT) >> % BT\nn q 1 0 0 1 7 9 cm BT ET Q BT ET BI BT ET";
    let target =
        budget(4 * (std::mem::size_of::<[f32; 6]>() + std::mem::size_of::<(usize, [f32; 6])>()));
    let result = text_matrices(&target, content, [1., 0., 0., 1., 0., 0.]);
    let first = content
        .windows(5)
        .position(|part| part == b"BT ET")
        .unwrap();
    let second = first
        + 5
        + content[first + 5..]
            .windows(5)
            .position(|part| part == b"BT ET")
            .unwrap();
    assert_eq!(
        result.entries,
        [
            (first, [1., 0., 0., 1., 7., 9.]),
            (second, [1., 0., 0., 1., 0., 0.])
        ]
    );
    assert!(result.get(&0).is_none());
    target.check().unwrap();
}

/// 完成したBT対応の後の拡張失敗でも部分対応を公開しない。
#[test]
fn later_matrix_growth_discards_completed_positions() {
    let target = budget(4 * std::mem::size_of::<(usize, [f32; 6])>());
    assert!(
        text_matrices(
            &target,
            b"BT ET BT ET BT ET BT ET BT ET",
            [1., 0., 0., 1., 0., 0.]
        )
        .is_empty()
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// q/Qで空になった列は容量を再利用し、解放後の再作成は累積へ数える。
#[test]
fn graphics_capacity_is_reused_and_recreation_is_cumulative() {
    let content = b"q Q q Q q q q q Q Q Q Q BT ET";
    let one = 4 * (std::mem::size_of::<[f32; 6]>() + std::mem::size_of::<(usize, [f32; 6])>());
    for capacity in [one - 1, one, 2 * one - 1, 2 * one] {
        let target = budget(capacity);
        for pass in 0..2 {
            let result = text_matrices(&target, content, [1., 0., 0., 1., 0., 0.]);
            assert_eq!(
                result.entries.len(),
                usize::from((pass + 1) * one <= capacity)
            );
        }
        assert_eq!(target.check().is_ok(), 2 * one <= capacity);
    }
}

/// 深度超過を先に報告し、完成済みBT対応を破棄する。
#[test]
fn graphics_depth_error_discards_positions_and_remains_first() {
    let target = DecodeBudget::new(&ParseLimits {
        max_container_nesting: 1,
        ..Default::default()
    });
    assert!(text_matrices(&target, b"BT ET q q", [1., 0., 0., 1., 0., 0.]).is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "Form graphics state depth",
            ..
        })
    ));
    assert!(text_matrices(&target, b"BT ET", [1., 0., 0., 1., 0., 0.]).is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 任意の平行移動・q/Q・BT列で独立した座標/高水位容量モデルと一致する。
    #[test]
    fn matrices_match_translation_and_capacity_model(
        operations in prop::collection::vec((0u8..5, -50i16..50, -50i16..50), 0..72),
        builds in 1usize..4,
        short in any::<bool>(),
    ) {
        let mut content = String::new();
        let mut current = (3., 5.);
        let mut saved = Vec::new();
        let mut peak = 0;
        let mut expected = Vec::new();
        for (operation, dx, dy) in operations {
            match operation {
                0 => { content.push_str("q "); saved.push(current); peak = peak.max(saved.len()); }
                1 => { content.push_str("Q "); if let Some(previous) = saved.pop() { current = previous; } }
                2 => { content.push_str(&format!("1 0 0 1 {dx} {dy} cm ")); current.0 += f32::from(dx); current.1 += f32::from(dy); }
                3 => { expected.push((content.len(), [1., 0., 0., 1., current.0, current.1])); content.push_str("BT ET "); }
                _ => content.push_str("(BT q) /BT [BT] << /K (BT) >> % BT\nn "),
            }
        }
        let one = slots(peak) * std::mem::size_of::<[f32; 6]>() + slots(expected.len()) * std::mem::size_of::<(usize, [f32; 6])>();
        let capacity = (one * builds).saturating_sub(usize::from(short));
        let target = budget(capacity);
        for pass in 0..builds {
            let result = text_matrices(&target, content.as_bytes(), [1., 0., 0., 1., 3., 5.]);
            if (pass + 1) * one <= capacity {
                prop_assert_eq!(&result.entries, &expected);
                for (offset, matrix) in &expected { prop_assert_eq!(result.get(offset), Some(matrix)); }
            } else { prop_assert!(result.is_empty()); }
        }
        prop_assert_eq!(target.check().is_ok(), one * builds <= capacity);
    }
}

/// CTM変更点の配列拡張も全要求容量を累積し、失敗時は先行対応を破棄する。
#[test]
fn matrix_changes_reserve_inclusive_cumulative_capacity() {
    let content =
        b"BT 1 0 0 1 1 0 cm 1 0 0 1 1 0 cm 1 0 0 1 1 0 cm 1 0 0 1 1 0 cm 1 0 0 1 1 0 cm (A) Tj ET";
    let required = (4 + 8) * std::mem::size_of::<(usize, [f32; 6])>();
    for capacity in [required - 1, required] {
        let target = budget(capacity);
        let matrices = text_matrices(&target, content, [1., 0., 0., 1., 0., 0.]);
        if capacity == required {
            assert_eq!(
                matrices.get(&content.len()),
                Some(&[1., 0., 0., 1., 5., 0.])
            );
            assert_eq!(matrices.entries.len(), 6);
            target.check().unwrap();
        } else {
            assert!(matrices.is_empty());
            assert!(target.check().is_err());
            assert!(text_matrices(&target, b"BT ET", [1., 0., 0., 1., 0., 0.]).is_empty());
        }
    }
}

/// 非有限になった行列は直前の有効な変更点へ誤って戻さず描画不能にする。
#[test]
fn nonfinite_change_does_not_reuse_earlier_matrix() {
    let target = budget(4 * std::mem::size_of::<(usize, [f32; 6])>());
    let matrices = text_matrices(
        &target,
        b"BT 2 0 0 2 0 0 cm (A) Tj ET",
        [f32::MAX, 0., 0., 1., 0., 0.],
    );
    assert!(matrices.get(&0).is_some());
    assert!(matrices.get(&99).is_none());
    target.check().unwrap();
}
