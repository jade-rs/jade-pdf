//! 元属性の優先順位・小数保持・有効世代・中止と丸め境界を検査する。
use super::*;
use crate::{
    ParseLimits,
    page_attributes::Objects,
    type1_outline::{file, header, permission},
};
use proptest::prelude::*;

/// 完全に所有の二層fontを読み、原本を借用したまま検査する。
fn with_font(f: impl FnOnce(Font<'_>)) {
    let (program, first, second, _) = crate::type1_embedding_tests::program();
    f(file::read(&DecodeBudget::default(), &program, first, second).unwrap());
}

/// 参照のないdescriptorを、同じ元fontと共通の予算で読む。
fn direct(budget: &DecodeBudget, font: &Font<'_>, dictionary: &[u8]) -> Option<Attributes> {
    let objects = Objects::default();
    let mut resolver = ResourceResolver::new(&objects, budget);
    read(budget, font, dictionary, &mut resolver)
}

/// 行metricはPDF、外形styleは元FontInfoを優先し、小数も失わない。
#[test]
fn descriptor_line_metrics_and_intrinsic_font_style_are_distinct() {
    with_font(|mut font| {
        font.header.info = header::Info {
            weight: Some(b"Bo\\154d"),
            italic_angle: Some(-12.5),
            fixed: Some(true),
            ascent: Some(750.),
            descent: Some(-250.),
            underline_position: Some(-95.5),
            underline_thickness: Some(42.25),
            ..Default::default()
        };
        let actual=direct(&DecodeBudget::default(),&font,b"<< /Ascent 800.25 /Descent -225.75 /XHeight 451.5 /CapHeight 701.25 /FontWeight 500 /ItalicAngle 0 /Flags 32 >>").unwrap();
        assert_eq!(
            (
                actual.ascent,
                actual.descent,
                actual.x_height,
                actual.cap_height
            ),
            (800.25, -225.75, 451.5, 701.25)
        );
        assert_eq!(actual.weight, 700);
        assert!(actual.bold && actual.italic && actual.fixed);
        assert_eq!(actual.italic_angle, -12.5);
        assert_eq!(
            (actual.underline_position, actual.underline_thickness),
            (-95.5, 42.25)
        );
    });
}

/// 未指定行metricは元FontInfo、さらに実外形へ戻し、名前に含むBoldをstyleにしない。
#[test]
fn missing_metrics_use_original_geometry_without_invented_style_or_underline() {
    with_font(|mut font| {
        font.header.name = b"UnrelatedBoldItalicName";
        font.header.info = header::Info::default();
        let actual = direct(&DecodeBudget::default(), &font, b"<< >>").unwrap();
        assert_eq!((actual.ascent, actual.descent), (700., 0.));
        assert_eq!(actual.weight, 400);
        assert!(!actual.bold && !actual.italic && !actual.fixed);
        assert_eq!(
            (
                actual.underline_position,
                actual.underline_thickness,
                actual.x_height,
                actual.cap_height
            ),
            (0., 0., 0., 0.)
        );
        font.header.info.ascent = Some(725.5);
        font.header.info.descent = Some(-225.25);
        font.header.info.italic_angle = Some(0.);
        font.header.info.fixed = Some(false);
        let inherited = direct(
            &DecodeBudget::default(),
            &font,
            b"<< /ItalicAngle -15 /Flags 65 /FontWeight 650 >>",
        )
        .unwrap();
        assert_eq!(
            (inherited.ascent, inherited.descent, inherited.weight),
            (725.5, -225.25, 650)
        );
        assert!(!inherited.italic && !inherited.fixed && !inherited.bold);
    });
}

/// descriptorが無い場合も、元外形を整数へ早く丸めて小数の高さを失わない。
#[test]
fn fallback_line_and_cap_metrics_keep_original_fractional_outline_bounds() {
    with_font(|mut font| {
        use crate::type1_outline::charstring::Command;
        font.header.info = header::Info::default();
        font.glyphs[1].name = b"H".to_vec();
        font.glyphs[1].outline.commands = vec![
            Command::Move([0., -123.25]),
            Command::Line([300., 700.25]),
            Command::Line([600., -123.25]),
            Command::Close,
        ];
        let actual = direct(&DecodeBudget::default(), &font, b"<< >>").unwrap();
        assert_eq!(
            (actual.ascent, actual.descent, actual.cap_height),
            (700.25, -123.25, 700.25)
        );
    });
}

/// 元三次曲線の極値から高さを補い、制御点の箱や整数外接枠へ置き換えない。
#[test]
fn fallback_metrics_use_fractional_cubic_extrema() {
    with_font(|mut font| {
        use crate::type1_outline::charstring::Command;
        font.header.info = header::Info::default();
        font.glyphs[1].name = b"x".to_vec();
        font.glyphs[1].outline.commands = vec![
            Command::Move([0., 0.]),
            Command::Curve([0., 1000.333, 1000., 1000.333, 1000., 0.]),
            Command::Line([0., 0.]),
            Command::Close,
        ];
        let actual = direct(&DecodeBudget::default(), &font, b"<< >>").unwrap();
        assert!((actual.ascent - 750.24975).abs() < 1e-9);
        assert_eq!(actual.ascent, actual.x_height);
    });
}

/// 明示Mediumを500として保持し、escape復号後の既知weightだけを認識する。
#[test]
fn declared_weight_classes_are_retained_without_substring_guesses() {
    with_font(|mut font| {
        for (raw, expected) in [
            (b"Thin".as_slice(), 100),
            (b"ExtraLight", 200),
            (b"Light", 300),
            (b"Book", 400),
            (b"Regular", 400),
            (b"Medium", 500),
            (b"Semi-Bold", 600),
            (b"Bo\\154d", 700),
            (b"ExtraBold", 800),
            (b"Black", 900),
            (b"NotBold", 400),
        ] {
            font.header.info.weight = Some(raw);
            let actual = direct(&DecodeBudget::default(), &font, b"<< >>").unwrap();
            assert_eq!(actual.weight, expected);
            assert_eq!(actual.bold, expected >= 700);
        }
    });
}

/// 壊れた参照・重複・非有限値・未対応matrixや停止で部分属性を返さない。
#[test]
fn invalid_or_stopped_inputs_do_not_publish_partial_attributes() {
    with_font(|mut font| {
        for raw in [
            b"<< /Ascent 700 /Ascent 800 >>".as_slice(),
            b"<< /Ascent NaN >>",
            b"<< /Ascent -1 >>",
            b"<< /Descent 1 >>",
            b"<< /CapHeight -1 >>",
            b"<< /XHeight -1 >>",
            b"<< /FontWeight 500.5 >>",
            b"<< /Flags -1 >>",
            b"<< /Ascent 4 0 R >>",
        ] {
            assert!(
                direct(&DecodeBudget::default(), &font, raw).is_none(),
                "{raw:?}"
            );
        }
        let stopped = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: 0,
            ..Default::default()
        });
        assert!(stopped.font_vector::<u8>(1).is_none());
        assert!(direct(&stopped, &font, b"<< >>").is_none());
        font.header.matrix = [0.002, 0., 0., 0.001, 0., 0.];
        assert!(direct(&DecodeBudget::default(), &font, b"<< >>").is_none());
    });
}

/// descriptorの間接数値は現行世代だけを使い、古い世代へ戻らない。
#[test]
fn indirect_descriptor_metrics_require_the_current_generation() {
    with_font(|font| {
        let objects = [(9, (2, b"812.25".as_slice()))]
            .into_iter()
            .collect::<Objects>();
        let budget = DecodeBudget::default();
        let mut resolver = ResourceResolver::new(&objects, &budget);
        let actual = read(&budget, &font, b"<< /Ascent 9 2 R >>", &mut resolver).unwrap();
        assert_eq!(actual.ascent, 812.25);
        assert!(read(&budget, &font, b"<< /Ascent 9 1 R >>", &mut resolver).is_none());
    });
}

/// PS weightは復号後の四byteだけ予約し、出力には元の権利表示をそのまま渡す。
#[test]
fn exact_weight_storage_and_scaled_output_keep_permission_and_fractional_metrics() {
    with_font(|mut font| {
        font.header.info.weight = Some(b"Bo\\154d");
        for limit in [4, 3] {
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_embedded_font_bytes: limit,
                ..Default::default()
            });
            assert_eq!(direct(&budget, &font, b"<< >>").is_some(), limit == 4);
        }
        let budget = DecodeBudget::default();
        let attrs = direct(
            &budget,
            &font,
            b"<< /Ascent 700.25 /Descent -200.25 /XHeight 450.25 /CapHeight 680.25 >>",
        )
        .unwrap();
        let license = permission::License {
            declared_flags: Some(8),
            flags: 8,
            basis: permission::Basis::FsType,
            notice: b"exact owned notice".to_vec(),
            text: "exact owned license".into(),
            url: "https://example.invalid/owned",
        };
        let info = attrs.to_info(4000, &[], &[], &license).unwrap();
        assert_eq!(
            (info.ascent, info.descent, info.x_height, info.cap_height),
            (2801, -801, 1801, 2721)
        );
        assert_eq!(
            (info.weight, info.bold, info.embedding_flags),
            (700, true, 8)
        );
        assert_eq!(info.license, license.text);
        assert_eq!(info.license_url, license.url);
        assert!(attrs.to_info(15, &[], &[], &license).is_none());
        assert!(attrs.to_info(16385, &[], &[], &license).is_none());
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 元小数の行metricを保ち、OpenType丸め誤差を半単位以内に収める。
    #[test]
    fn fractional_source_metrics_round_only_at_the_selected_units(ascent in 1u16..1500,descent in 0u16..400,quarter in 0u16..4,units in 16u16..16000) {
        with_font(|font| {
            let a=f64::from(ascent)+f64::from(quarter)/4.;let d=-(f64::from(descent)+f64::from(quarter)/4.);
            let raw=format!("<< /Ascent {a} /Descent {d} >>");let attrs=direct(&DecodeBudget::default(),&font,raw.as_bytes()).unwrap();
            assert_eq!((attrs.ascent,attrs.descent),(a,d));
            let license=permission::License{declared_flags:Some(0),flags:0,basis:permission::Basis::FsType,notice:vec![],text:String::new(),url:""};
            let info=attrs.to_info(units,&[],&[],&license).unwrap();let scale=f64::from(units)/1000.;
            assert!((f64::from(info.ascent)/scale-a).abs()<=0.5/scale+1e-10);
            assert!((f64::from(info.descent)/scale-d).abs()<=0.5/scale+1e-10);
        });
    }
}
