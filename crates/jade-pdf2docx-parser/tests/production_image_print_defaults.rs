//! 原本画像のsampleを、画像へ作用しない通常の印刷設定によって白へ置換しない。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{QualityIssue, parse_pdf_bytes};
use proptest::prelude::*;

/// 画像の非stroke alphaや色変換を追加しない、通常の印刷状態。
fn resources() -> &'static str {
    "<< /XObject << /Im 4 0 R >> /ExtGState << /S << /Type /ExtGState /BG2 /Default /UCR2 /Default /OP false /op false /OPM 1 /SA false /SM .02 >> >> >>"
}

/// DeviceRGB画像は通常の印刷設定でも、固有の六つのsource sampleを保持する。
#[test]
fn normal_print_defaults_preserve_color_image_samples() {
    let input = fixture::image_with_resources(
        "/ColorSpace /DeviceRGB /Filter /FlateDecode",
        &fixture::compressed(&[12, 34, 56, 78, 90, 123]),
        8,
        b"/S gs /Im Do",
        resources(),
        false,
        &[],
    );
    let parsed = parse_pdf_bytes("owned-normal-print-rgb.pdf", &input).unwrap();
    assert!(
        !parsed
            .quality
            .issues
            .iter()
            .any(|q| matches!(q, QualityIssue::ImageReplaced { .. })),
        "{:?}",
        parsed.quality
    );
    assert_eq!(parsed.image_elements.len(), 1);
    assert_eq!(
        fixture::scanline(&parsed.image_elements[0].png_bytes),
        [0, 12, 34, 56, 78, 90, 123]
    );
}

/// stencilの二つのsource bitは、通常の印刷設定でも指定した塗り色と透明部を保持する。
#[test]
fn normal_print_defaults_preserve_stencil_color_and_alpha() {
    let input = fixture::stencil_with_resources(
        &fixture::compressed(&[0x40]),
        b".25 .5 .75 rg /S gs /Im Do",
        resources(),
        true,
        &[],
    );
    let parsed = parse_pdf_bytes("owned-normal-print-stencil.pdf", &input).unwrap();
    assert!(
        !parsed
            .quality
            .issues
            .iter()
            .any(|q| matches!(q, QualityIssue::ImageReplaced { .. })),
        "{:?}",
        parsed.quality
    );
    assert_eq!(parsed.image_elements.len(), 1);
    assert_eq!(
        fixture::scanline(&parsed.image_elements[0].png_bytes),
        [0, 64, 128, 191, 255, 64, 128, 191, 0]
    );
}

/// 間接・escaped属性も同じ所属Resourceで解決し、caを一度だけ適用する。
#[test]
fn indirect_print_defaults_preserve_nonstroking_alpha() {
    let input = fixture::image_with_resources(
        "/ColorSpace /DeviceRGB /Filter /FlateDecode",
        &fixture::compressed(&[12, 34, 56, 78, 90, 123]),
        8,
        b"1 0 0 0 k /S gs /Im Do",
        "<< /XObject << /Im 4 0 R >> /ExtGState << /S 12 0 R >> >>",
        true,
        &[
            b"<< /B#472 13 0 R /UCR2 /Def#61ult /S#41 false /S#4d 14 0 R /ca .5 >>",
            b"/Default",
            b".02",
        ],
    );
    let parsed = parse_pdf_bytes("owned-indirect-print.pdf", &input).unwrap();
    assert!(
        !parsed
            .quality
            .issues
            .iter()
            .any(|q| matches!(q, QualityIssue::ImageReplaced { .. }))
    );
    assert_eq!(
        fixture::scanline(&parsed.image_elements[0].png_bytes),
        [0, 12, 34, 56, 128, 78, 90, 123, 128]
    );
}

/// 任意関数・範囲外・重複・不正な型は拒否し、後の通常設定でも未確認を解除しない。
#[test]
fn unknown_print_effects_stay_unconfirmed_until_graphics_restore() {
    for bad in [
        "/BG2 /Identity",
        "/BG2 << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>",
        "/UCR2 null",
        "/UCR2 /Identity",
        "/SM -.1",
        "/SM 1.0000000000000001",
        "/SM [.02]",
        "/SM .02 /S#4d .03",
        "/SA 0",
        "/BG2 /Default /B#472 /Default",
        "/TR /Identity",
    ] {
        let resource = format!(
            "<< /XObject << /Im 4 0 R >> /ExtGState << /Bad << {bad} >> /Safe << /BG2 /Default /UCR2 /Default /SA false /SM .02 >> >> >>"
        );
        let input = fixture::image_with_resources(
            "/ColorSpace /DeviceRGB /Filter /FlateDecode",
            &fixture::compressed(&[12, 34, 56, 78, 90, 123]),
            8,
            b"q /Bad gs /Im Do /Safe gs /Im Do Q /Im Do",
            &resource,
            false,
            &[],
        );
        let parsed = parse_pdf_bytes("owned-unknown-print.pdf", &input).unwrap();
        assert!(
            parsed
                .quality
                .issues
                .iter()
                .any(|q| matches!(q, QualityIssue::ImageReplaced { .. })),
            "{bad}"
        );
        assert_eq!(parsed.image_elements.len(), 3);
        assert_eq!(
            fixture::scanline(&parsed.image_elements[2].png_bytes),
            [0, 12, 34, 56, 78, 90, 123],
            "{bad}"
        );
        for image in &parsed.image_elements[..2] {
            assert_ne!(
                fixture::scanline(&image.png_bytes),
                [0, 12, 34, 56, 78, 90, 123],
                "{bad}"
            );
        }
    }
}

/// Device色がpageの既定色空間へ置換される場合、通常印刷設定だけでRGBを断定しない。
#[test]
fn print_defaults_do_not_bypass_unconfirmed_device_color_spaces() {
    for spaces in [
        "/ColorSpace << /DefaultRGB [/CalRGB << /WhitePoint [1 1 1] /Gamma [2 2 2] >>] >>",
        "/ColorSp#61ce << /Def#61ultRGB [/CalRGB << /WhitePoint [1 1 1] >>] >>",
    ] {
        let resource = format!(
            "<< /XObject << /Im 4 0 R >> {spaces} /ExtGState << /S << /BG2 /Default /UCR2 /Default /SA false /SM .02 >> >> >>"
        );
        let input = fixture::image_with_resources(
            "/ColorSpace /DeviceRGB /Filter /FlateDecode",
            &fixture::compressed(&[12, 34, 56, 78, 90, 123]),
            8,
            b"/S gs /Im Do",
            &resource,
            false,
            &[],
        );
        let parsed = parse_pdf_bytes("owned-default-color-print.pdf", &input).unwrap();
        assert!(
            parsed
                .quality
                .issues
                .iter()
                .any(|q| matches!(q, QualityIssue::ImageReplaced { .. })),
            "{spaces}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 有効なsmoothnessと両SA値を変えても、任意のRGB source sampleを変えない。
    #[test]
    fn print_defaults_do_not_change_rgb_samples(samples in any::<[u8;6]>(),smoothness in 0u16..=1024,adjust in any::<bool>()){
        let resource=format!("<< /XObject << /Im 4 0 R >> /ExtGState << /S << /BG2 /Default /UCR2 /Default /SA {adjust} /SM {:.10} >> >> >>",f64::from(smoothness)/1024.);
        let input=fixture::image_with_resources("/ColorSpace /DeviceRGB /Filter /FlateDecode",&fixture::compressed(&samples),8,b"/S gs /Im Do",&resource,false,&[]);
        let parsed=parse_pdf_bytes("owned-print-property.pdf",&input).unwrap();
        prop_assert!(!parsed.quality.issues.iter().any(|q|matches!(q,QualityIssue::ImageReplaced{..})),"source RGB image must not be replaced");
        let mut expected=vec![0];expected.extend_from_slice(&samples);
        prop_assert_eq!(fixture::scanline(&parsed.image_elements[0].png_bytes),expected);
    }
}
