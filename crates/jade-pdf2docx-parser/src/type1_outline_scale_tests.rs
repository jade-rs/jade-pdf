//! 所有の小数幅と大きい外形で、PDF幅優先と単位数の制限を検査する。
use super::*;
use crate::type1_outline::charstring::Command;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 元の幅は849、PDFが849.5と指定する、所有の線と曲線。
fn glyph() -> Glyph {
    Glyph {
        advance: 849.,
        commands: vec![
            Command::Move([10., 20.]),
            Command::Line([100., 200.]),
            Command::Curve([100., 300., 200., 400., 300., 200.]),
            Command::Close,
        ],
    }
}

/// 観測済みの0.1単位幅を正確に格納し、全値が整数ならより細かい単位を選ぶ。
#[test]
fn decimal_pdf_widths_choose_exact_legal_units() {
    let limits = Limits {
        coordinate: 1200.,
        delta: 1700.,
        metric: 1000.,
        advance: 1200.,
    };
    assert_eq!(
        choose_units(
            &DecodeBudget::default(),
            &[437.5, 799.8, 849.5, 862.3, 277.8],
            &limits
        ),
        Some(10000)
    );
    assert_eq!(
        choose_units(&DecodeBudget::default(), &[500., 600.], &limits),
        Some(16000)
    );
    let wide = Limits {
        advance: 65535.,
        ..limits
    };
    assert_eq!(
        choose_units(&DecodeBudget::default(), &[65535., 600.], &wide),
        Some(1000)
    );
}

/// advanceはPDF側で上書きし、輪郭は同じ比率だけ拡大して描画命令順を保つ。
#[test]
fn pdf_width_takes_priority_without_changing_the_outline_shape() {
    let source = glyph();
    let converted = prepare(&DecodeBudget::default(), &source, Some(849.5), 10000).unwrap();
    assert_eq!(converted.advance, 8495.);
    assert_eq!(source.advance, 849.);
    assert_eq!(
        converted.commands,
        vec![
            Command::Move([100., 200.]),
            Command::Line([1000., 2000.]),
            Command::Curve([1000., 3000., 2000., 4000., 3000., 2000.]),
            Command::Close
        ]
    );
    assert_eq!(
        prepare(&DecodeBudget::default(), &source, None, 10000)
            .unwrap()
            .advance,
        8490.
    );
    let stopped = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(prepare(&stopped, &source, Some(849.5), 10000).is_none());
    assert!(matches!(
        stopped.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            ..
        })
    ));
}

/// 不正幅と範囲外の外形を、飽和・既定値・font内の幅へ黙って置き換えない。
#[test]
fn invalid_or_unrepresentable_values_do_not_fallback() {
    for width in [f64::NAN, f64::INFINITY, -1., 65535.] {
        assert!(prepare(&DecodeBudget::default(), &glyph(), Some(width), 10000).is_none());
    }
    for units in [0, 15, 16385] {
        assert!(prepare(&DecodeBudget::default(), &glyph(), None, units).is_none());
    }
    let limits = Limits {
        coordinate: 1e9,
        delta: 1e9,
        metric: 1e9,
        advance: 1e9,
    };
    assert!(choose_units(&DecodeBudget::default(), &[600.], &limits).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意の小数第一位の幅も、選んだ単位で物理advanceを失わない。
    #[test]
    fn tenths_preserve_pdf_advance(value in 1_u32..12000) {
        let width=f64::from(value)/10.;let limits=Limits {coordinate:1200.,delta:1700.,metric:1000.,advance:1200.};
        let units=choose_units(&DecodeBudget::default(),&[width],&limits).unwrap();
        let prepared=prepare(&DecodeBudget::default(),&glyph(),Some(width),units).unwrap();
        prop_assert!((prepared.advance*1000./f64::from(units)-width).abs()<1e-8);
    }
}
