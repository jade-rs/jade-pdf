//! 段幅の境界・丸め・XML梱包の不変条件。
use super::*;
use proptest::prelude::*;

/// 非有限・逆転・ページ外・twip丸め後に空となる段領域を拒否する。
#[test]
fn invalid_frames_are_not_invented() {
    for (width, origin, left, right, font) in [
        (f32::NAN, 54., 289., 309., 10.),
        (612., f32::INFINITY, 289., 309., 10.),
        (612., 54., f32::NAN, 309., 10.),
        (612., 54., 289., f32::INFINITY, 10.),
        (612., 54., 289., 309., f32::NAN),
        (612., -1., 289., 309., 10.),
        (612., 54., 54., 309., 10.),
        (612., 54., 309., 309., 10.),
        (612., 54., 309., 289., 10.),
        (612., 54., 289., 612., 10.),
        (612., 54., 289., 309., 0.),
        (612., 54., 289., 309., -1.),
        (f32::MAX, 54., 289., 309., 10.),
        (612., 54., 289., 289.001, 10.),
        (612., 600., 601., 602., 10.),
    ] {
        assert_eq!(Frame::infer(width, origin, left, right, font), None);
    }
}

proptest! {
    /// 原本段間と字面幅が変わっても、Word段境界と符号付きindentで字面位置を保つ。
    #[test]
    fn varied_gutters_keep_frames_disjoint_and_source_coordinates(
        width in 650u32..1400,
        origin in 400u32..2000,
        ink in 2800u32..4600,
        source_gap in 240u32..1600,
        font in 160u32..360,
    ) {
        let origin = origin as f32 / 20.;
        let left_end = origin + ink as f32 / 20.;
        let right_start = left_end + source_gap as f32 / 20.;
        let frame = Frame::infer(width as f32, origin, left_end, right_start, font as f32 / 20.).unwrap();
        let first_end = frame.origin(width as f32, 1).unwrap() - f64::from(DEFAULT_DOCX_COLUMN_SPACE_TWIPS);
        prop_assert!(f64::from(left_end)*20. <= first_end);
        prop_assert!(first_end < f64::from(right_start)*20.);
        if let Some(widths) = frame.widths {
            prop_assert!(widths[0] > 0 && widths[1] > 0);
            prop_assert_eq!(frame.left + widths[0] + widths[1] + DEFAULT_DOCX_COLUMN_SPACE_TWIPS as u32, width*20);
        }
        for (column, x) in [(0, origin), (1, right_start)] {
            let actual_origin = frame.origin(width as f32, column).unwrap();
            let indent = crate::paragraph_anchor::column_indent(x, x+30., actual_origin, width as f32).unwrap();
            prop_assert!((actual_origin+f64::from(indent)-f64::from(x)*20.).abs() <= 0.5001);
        }
    }
}

/// 既に安全な均等段の本文XMLは、梱包時に一文字も変更しない。
#[test]
fn safe_equal_columns_keep_xml_bytes() {
    let mut xml = b"<w:document><w:cols w:space=\"425\" w:num=\"2\" /></w:document>".to_vec();
    let original = xml.clone();
    let frame = Frame::infer(612., 48., 113., 330., 11.).unwrap();
    assert!(frame.widths.is_none());
    apply(&mut xml, &[frame]).unwrap();
    assert_eq!(xml, original);
}

/// 本文sectionと寸法metadataの数が一致しなければ、書き換えずに拒否する。
#[test]
fn mismatched_column_frames_fail_without_partial_rewrite() {
    let frame = Frame::infer(612., 54., 289., 309., 10.).unwrap();
    for (xml, frames) in [
        (
            "<w:document><w:cols w:num=\"1\" /></w:document>",
            vec![frame.clone()],
        ),
        (
            "<w:document><w:cols w:num=\"2\" /><w:cols w:num=\"2\" /></w:document>",
            vec![frame.clone()],
        ),
        (
            "<w:document><w:cols w:num=\"2\" /></w:document>",
            vec![frame.clone(), frame.clone()],
        ),
    ] {
        let mut bytes = xml.as_bytes().to_vec();
        assert!(apply(&mut bytes, &frames).is_err());
        assert_eq!(bytes, xml.as_bytes());
    }
}
