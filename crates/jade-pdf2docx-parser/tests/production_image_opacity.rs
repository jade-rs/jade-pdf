//! 描画時の非stroke alphaを、画像固有のalphaと区別して検証する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// 直接・間接・継承状態のcaをstencilへ適用し、stroke用CAを無視する。
#[test]
fn stencil_opacity_uses_nonstroking_alpha_only() {
    for inherited in [false, true] {
        for (state, alpha) in [
            ("<< /ca .5 >>", 128),
            ("<< /c#61 13 0 R /CA .1 >>", 64),
            ("<< /ca 0 >>", 0),
            ("<< /ca -0.000 >>", 0),
            ("<< /ca .9999999999999999 >>", 255),
            ("<< /ca +01.000 >>", 255),
            ("<< /CA .5 >>", 255),
            ("<< /ca .5 /TK true >>", 128),
            ("<< /ca .5 /T#4b false >>", 128),
        ] {
            let pdf = fixture::stencil_with_resources(
                &fixture::compressed(&[0x40]),
                b".25 .5 .75 rg /S gs /Im Do",
                "<< /XObject << /Im 4 0 R >> /ExtGState << /S 12 0 R >> >>",
                inherited,
                &[state.as_bytes(), b".25"],
            );
            let document =
                parse_pdf_bytes_with_options("alpha.pdf", &pdf, &ParseOptions::default()).unwrap();
            assert_eq!(
                document.quality.issues,
                [QualityIssue::OcrRequired { page: 0 }],
                "{state}"
            );
            assert_eq!(
                fixture::scanline(&document.image_elements[0].png_bytes),
                [0, 64, 128, 191, alpha, 64, 128, 191, 0],
                "{state}"
            );
        }
    }
}

/// 通常画像では塗り色を使わず、SMaskへcaを一度だけ乗算して丸める。
#[test]
fn normal_image_opacity_multiplies_intrinsic_alpha_once() {
    for (mask, expected) in [("", [128, 128]), ("/SMask 5 0 R", [43, 128])] {
        let pdf = fixture::image_with_resources(
            &format!("/ColorSpace /DeviceGray /Filter /FlateDecode {mask}"),
            &fixture::compressed(&[64, 192]),
            8,
            b"1 0 0 0 k /S gs /Im Do",
            "<< /XObject << /Im 4 0 R >> /ExtGState << /S << /ca .5 >> >> >>",
            false,
            &[],
        );
        let document =
            parse_pdf_bytes_with_options("normal.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert_eq!(
            document.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        assert_eq!(
            fixture::scanline(&document.image_elements[0].png_bytes),
            [0, 64, 64, 64, expected[0], 192, 192, 192, expected[1]]
        );
    }
}

/// caの更新は置換であり、no-opとCAは保持し、q/Qは元の値を復元する。
#[test]
fn opacity_is_replaced_and_restored_per_draw() {
    let pdf = fixture::stencil_with_resources(
        &fixture::compressed(&[0x40]),
        b"/Half gs /Im Do q /Quarter gs /Im Do /Noop gs /Im Do /Stroke gs /Im Do Q /Im Do",
        "<< /XObject << /Im 4 0 R >> /ExtGState << /Half << /ca .5 >> /Quarter << /ca .25 >> /Noop << /OPM 1 >> /Stroke << /CA .1 >> >> >>",
        false,
        &[],
    );
    let document =
        parse_pdf_bytes_with_options("saved.pdf", &pdf, &ParseOptions::default()).unwrap();
    let alphas: Vec<_> = document
        .image_elements
        .iter()
        .map(|image| fixture::scanline(&image.png_bytes)[4])
        .collect();
    assert_eq!(alphas, [128, 64, 64, 64, 128]);
    assert_eq!(
        document.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
}

/// 未対応gsを通常画像でも黙って不透明として出力せず、置換を診断する。
#[test]
fn unresolved_graphics_state_is_reported_for_normal_images() {
    for state in [
        "<< /ca 1.0000000000000001 >>",
        "<< /ca -0.0000000000000001 >>",
        "<< /BM /Multiply >>",
        "<< /ca .5 /c#61 .5 >>",
    ] {
        let pdf = fixture::image_with_resources(
            "/ColorSpace /DeviceGray /Filter /FlateDecode",
            &fixture::compressed(&[64, 192]),
            8,
            b"/S gs /Im Do",
            "<< /XObject << /Im 4 0 R >> /ExtGState << /S 12 0 R >> >>",
            false,
            &[state.as_bytes()],
        );
        let document =
            parse_pdf_bytes_with_options("bad.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert!(
            document
                .quality
                .issues
                .iter()
                .any(|issue| matches!(issue, QualityIssue::ImageReplaced { .. })),
            "{state}"
        );
        assert!(
            matches!(parse_pdf_bytes_with_options("bad.pdf", &pdf, &ParseOptions { strict: true, ..Default::default() }),
            Err(jade_pdf2docx_parser::ParseError::QualityRejected {report, ..}) if report.issues.iter().any(|issue| matches!(issue, QualityIssue::ImageReplaced {..})))
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 二進で厳密なalphaと画像alphaの積を整数oracleで照合し、事前量子化を防ぐ。
    #[test]
    fn decimal_opacity_multiplies_before_rounding(n in 1u32..1024) {
        let fraction = u64::from(n) * 9_765_625;
        let resources = format!("<< /XObject << /Im 4 0 R >> /ExtGState << /S << /ca .{fraction:010} >> >> >>");
        let pdf = fixture::image_with_resources(
            "/ColorSpace /DeviceGray /Filter /FlateDecode /SMask 5 0 R", &fixture::compressed(&[64,192]), 8,
            b"/S gs /Im Do", &resources, false, &[],
        );
        let document = parse_pdf_bytes_with_options("property.pdf", &pdf, &ParseOptions::default()).unwrap();
        prop_assert_eq!(&document.quality.issues, &[QualityIssue::OcrRequired { page: 0 }]);
        let row = fixture::scanline(&document.image_elements[0].png_bytes);
        prop_assert_eq!(row, vec![0,64,64,64,((85*n+512)/1024) as u8,192,192,192,((255*n+512)/1024) as u8]);
    }
}
