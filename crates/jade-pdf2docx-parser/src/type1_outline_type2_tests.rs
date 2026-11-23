//! 所有の文字幅・輪郭・数値境界だけで、Type2への格納を検査する。
use super::*;
use crate::type1_outline::charstring::Command;
use crate::{ParseCancellation, ParseError, ParseLimits};
use proptest::prelude::*;

/// 空の外形でも文字幅を持つglyph。
fn empty(advance: f64) -> Glyph {
    Glyph {
        advance,
        commands: Vec::new(),
    }
}

/// 大きい幅と空文字を、Type2のwidth差分とendcharへ格納する。
#[test]
fn empty_glyph_width_uses_the_full_u16_range() {
    assert_eq!(NOMINAL_WIDTH, 32768);
    for (width, bytes) in [
        (0., vec![28, 128, 0, 14]),
        (600., vec![28, 130, 88, 14]),
        (65535., vec![28, 127, 255, 14]),
    ] {
        assert_eq!(encode(&DecodeBudget::default(), &empty(width)), Some(bytes));
    }
}

/// closeは原点を戻さず、後のmoveと三次曲線は絶対座標どおりに相対化する。
#[test]
fn contours_and_fractional_curve_keep_independent_control_points() {
    let glyph = Glyph {
        advance: 32768.,
        commands: vec![
            Command::Move([0., 0.]),
            Command::Line([100., 0.]),
            Command::Close,
            Command::Move([100.5, 10.]),
            Command::Curve([101., 10., 101., 20., 100.5, 20.]),
            Command::Close,
        ],
    };
    assert_eq!(
        encode(&DecodeBudget::default(), &glyph),
        Some(vec![
            139, 139, 139, 21, 239, 139, 5, 255, 0, 0, 128, 0, 149, 21, 255, 0, 0, 128, 0, 139,
            139, 149, 255, 255, 255, 128, 0, 139, 8, 14,
        ])
    );
}

/// 正確な最終byte数で確保し、容量不足と取消で部分programを渡さない。
#[test]
fn output_storage_is_exact_and_cumulative() {
    let target = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 4,
        ..Default::default()
    });
    assert_eq!(encode(&target, &empty(600.)), Some(vec![28, 130, 88, 14]));
    assert!(encode(&target, &empty(600.)).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            actual: 8,
            ..
        })
    ));
    let target = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 3,
        ..Default::default()
    });
    assert!(encode(&target, &empty(600.)).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            actual: 4,
            ..
        })
    ));
    let token = ParseCancellation::new();
    token.cancel();
    let target = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(encode(&target, &empty(600.)).is_none());
    assert!(matches!(target.check(), Err(ParseError::Cancelled)));
}

/// 不正な輪郭・非有限値・表現できない差分を、黙って飽和や省略しない。
#[test]
fn invalid_geometry_and_unrepresentable_delta_are_rejected() {
    for width in [-1., 65536., f64::NAN, f64::INFINITY] {
        assert!(encode(&DecodeBudget::default(), &empty(width)).is_none());
    }
    for commands in [
        vec![Command::Close],
        vec![Command::Line([0., 0.])],
        vec![Command::Move([0., 0.])],
        vec![
            Command::Move([0., 0.]),
            Command::Move([1., 1.]),
            Command::Close,
        ],
        vec![Command::Move([f64::NAN, 0.]), Command::Close],
        vec![
            Command::Move([-32767., 0.]),
            Command::Line([32767., 0.]),
            Command::Close,
        ],
    ] {
        assert!(
            encode(
                &DecodeBudget::default(),
                &Glyph {
                    advance: 600.,
                    commands
                }
            )
            .is_none()
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意u16幅を、空文字としても失わずに有限byteへ格納する。
    #[test]
    fn every_u16_width_has_a_program(width in any::<u16>()) {
        let bytes=encode(&DecodeBudget::default(),&empty(f64::from(width))).unwrap();
        prop_assert!(bytes.len()<=6);prop_assert_eq!(bytes.last(),Some(&14));
    }
}
