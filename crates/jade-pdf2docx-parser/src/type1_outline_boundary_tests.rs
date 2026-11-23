//! 完了・保存量・取消・不正入力の境界を、所有glyphだけで検査する。
use super::*;
use crate::{ParseCancellation, ParseError, ParseLimits};
use proptest::prelude::*;

/// 二回目の保存配列確保が必要な、既知の五命令の三角形。
const TRIANGLE: &[u8] = &[
    139, 248, 236, 13, 139, 139, 21, 239, 139, 5, 139, 239, 5, 39, 39, 5, 9, 14,
];

/// 空glyphは動的保存不要で、描画glyphは最終確保を含めて上限検査する。
#[test]
fn geometry_storage_is_cumulative_and_empty_glyph_needs_no_storage() {
    let empty_budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert_eq!(
        decode(&empty_budget, &[139, 248, 236, 13, 14], &[]),
        Some(Glyph {
            advance: 600.,
            commands: Vec::new()
        })
    );
    // 旧容量4と新容量8の両方を、解放で差し戻さずに計上する。
    let required = (12 * std::mem::size_of::<Command>()) as u64;
    let exact = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: required,
        ..Default::default()
    });
    assert_eq!(decode(&exact, TRIANGLE, &[]).unwrap().commands.len(), 5);
    let short = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: required - 1,
        ..Default::default()
    });
    assert!(decode(&short, TRIANGLE, &[]).is_none());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { resource: "embedded font bytes", actual, .. }) if actual == required)
    );
    assert!(decode(&short, &[139, 139, 13, 14], &[]).is_none());
}

/// 解析前の取消も、外形完成後の取消も部分glyphを公開しない。
#[test]
fn cancellation_before_or_after_geometry_prevents_publication() {
    let token = ParseCancellation::new();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    let mut state = interpreter::State::new(&budget);
    state.run(TRIANGLE, &[], 0).unwrap();
    token.cancel();
    assert!(state.finish().is_none());
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
    assert!(decode(&budget, TRIANGLE, &[]).is_none());
}

/// stack過多・無限再帰・命令数超過・不正数値・幅と終端の混同を拒否する。
#[test]
fn execution_limits_and_malformed_numbers_fail_closed() {
    let mut long = vec![139, 248, 236, 13];
    for _ in 0..100_000 {
        long.extend_from_slice(&[139, 139, 1]);
    }
    long.push(14);
    let mut stack_overflow = vec![139; 49];
    stack_overflow.push(14);
    for bytes in [
        long,
        stack_overflow,
        vec![255, 0, 0, 1],
        vec![247],
        vec![139, 139, 12, 12, 13, 14],
        vec![139, 139, 13, 139, 139, 13, 14],
        vec![139, 139, 13, 139, 139, 21, 14, 14],
        vec![139, 139, 13, 139, 139, 21, 139, 10, 14],
    ] {
        assert!(decode(&DecodeBudget::default(), &bytes, &[vec![139, 10, 11]]).is_none());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 完成したglyphのすべての厳密prefixは、完成結果として受け取らない。
    #[test]
    fn truncated_owned_geometry_is_never_published(length in 0_usize..TRIANGLE.len()) {
        prop_assert!(decode(&DecodeBudget::default(), &TRIANGLE[..length], &[]).is_none());
    }

    /// 任意byte列はpanicせず、成功時も座標・advance・輪郭数が有限である。
    #[test]
    fn arbitrary_programs_are_bounded(bytes in proptest::collection::vec(any::<u8>(), 0..1024)) {
        if let Some(glyph) = decode(&DecodeBudget::default(), &bytes, &[]) {
            prop_assert!((0. ..=65535.).contains(&glyph.advance));
            prop_assert!(glyph.commands.len() <= bytes.len());
            for command in glyph.commands {
                let points: &[f64] = match &command {
                    Command::Move(p) | Command::Line(p) => p,
                    Command::Curve(p) => p,
                    Command::Close => &[],
                };
                prop_assert!(points.iter().all(|n| n.is_finite() && n.abs() <= 32767.));
            }
        }
    }
}
