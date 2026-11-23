//! 所有の曲線と非有限値で、実輪郭の範囲とType2量子化の一致を検査する。
use super::*;
use crate::type1_outline::charstring::Command;
use crate::{ParseCancellation, ParseLimits};
use proptest::prelude::*;

/// 所有の命令列を幅600のglyphへ包む。
fn glyph(commands: Vec<Command>) -> Glyph {
    Glyph {
        advance: 600.,
        commands,
    }
}

/// 制御点y=100でも実曲線の最大は75で、移動だけの輪郭は空として扱う。
#[test]
fn cubic_extrema_and_empty_contours_are_measured() {
    let source = glyph(vec![
        Command::Move([0., 0.]),
        Command::Curve([0., 100., 100., 100., 100., 0.]),
        Command::Close,
        Command::Move([1000., 1000.]),
        Command::Close,
    ]);
    let metric = measure(&DecodeBudget::default(), &source).unwrap();
    assert_eq!(metric.bounds, Some([0, 0, 100, 75]));
    assert_eq!(metric.left, 0);
    assert_eq!(metric.advance, 600);
    assert_eq!(
        measure(&DecodeBudget::default(), &glyph(vec![]))
            .unwrap()
            .bounds,
        None
    );
    let line = glyph(vec![
        Command::Move([-10.25, -2.75]),
        Command::Line([20.25, 3.25]),
        Command::Close,
    ]);
    let metric = measure(&DecodeBudget::default(), &line).unwrap();
    assert_eq!(metric.bounds, Some([-11, -3, 21, 4]));
    assert_eq!(metric.left, -10);
}

/// headerは元の浮動座標ではなく、保存される16.16座標の範囲を表す。
#[test]
fn fixed_point_coordinates_and_degenerate_cubics_are_consistent() {
    let source = glyph(vec![
        Command::Move([1. - 1e-8, -1. + 1e-8]),
        Command::Curve([1. - 1e-8, 0., 2., 0., 2., 0.]),
        Command::Close,
    ]);
    assert_eq!(
        measure(&DecodeBudget::default(), &source).unwrap().bounds,
        Some([1, -1, 2, 0])
    );
    let line = glyph(vec![
        Command::Move([0., 0.]),
        Command::Curve([1., 1., 2., 2., 3., 3.]),
        Command::Close,
    ]);
    assert_eq!(
        measure(&DecodeBudget::default(), &line).unwrap().bounds,
        Some([0, 0, 3, 3])
    );
}

/// 不正な輪郭・幅・座標と取消を、空glyphの成功に置き換えない。
#[test]
fn invalid_geometry_and_cancellation_are_rejected() {
    for commands in [
        vec![Command::Close],
        vec![Command::Line([0., 0.])],
        vec![Command::Move([0., 0.])],
        vec![Command::Move([f64::NAN, 0.]), Command::Close],
        vec![Command::Move([32768., 0.]), Command::Close],
        vec![
            Command::Move([-32767., 0.]),
            Command::Line([32767., 0.]),
            Command::Close,
        ],
    ] {
        assert!(measure(&DecodeBudget::default(), &glyph(commands)).is_none());
    }
    for advance in [-1., f64::NAN, 65536.] {
        let mut source = glyph(vec![]);
        source.advance = advance;
        assert!(measure(&DecodeBudget::default(), &source).is_none());
    }
    let token = ParseCancellation::new();
    token.cancel();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(measure(&budget, &glyph(vec![])).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意の所有三次曲線の全標本が、外向きに丸めた範囲へ収まる。
    #[test]
    fn sampled_cubic_stays_inside_measured_bounds(points in prop::array::uniform8(-1000_i16..1000)) {
        let p=points.map(f64::from);
        let source=glyph(vec![Command::Move([p[0],p[1]]),Command::Curve([p[2],p[3],p[4],p[5],p[6],p[7]]),Command::Close]);
        let b=measure(&DecodeBudget::default(),&source).unwrap().bounds.unwrap();
        for step in 0..=64 {
            let t=f64::from(step)/64.;let u=1.-t;
            for axis in 0..2 {
                let value=u*u*u*p[axis]+3.*u*u*t*p[axis+2]+3.*u*t*t*p[axis+4]+t*t*t*p[axis+6];
                prop_assert!(value>=f64::from(b[axis])-1e-9 && value<=f64::from(b[axis+2])+1e-9);
            }
        }
    }
}
