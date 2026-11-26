//! 回転後の表示矩形で可視範囲とstrictのOCR確認要求を判定する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
#[cfg(not(target_arch = "wasm32"))]
#[path = "support/paths.rs"]
mod paths;
use jade_pdf2docx_parser::{
    ParseError, ParseOptions, QualityIssue, image_requires_ocr, parse_pdf_bytes,
    parse_pdf_bytes_with_options, text_is_visible,
};
use proptest::prelude::*;

/// 同じ非正方形ページで、回転後の右端帯または下端外へ二画素画像を置く。
fn input(rotation: i32, visible: bool) -> Vec<u8> {
    let matrix = match (rotation, visible) {
        (90, true) => "20 0 0 10 10 250",
        (90, false) => "20 0 0 10 -80 20",
        (270, true) => "20 0 0 10 10 20",
        (270, false) => "20 0 0 10 250 20",
        _ => unreachable!(),
    };
    let content = format!("q {matrix} cm /Im Do Q BT /F1 12 Tf 10 50 Td (retained text) Tj ET");
    fixture::rgb_with_page_geometry(200, 300, rotation, content.as_bytes())
}

/// 表示ページ内の画像を元の幅で除外せず、strictで未確認画像を成功へ格下げしない。
#[test]
fn visible_image_in_rotated_right_strip_keeps_ocr() {
    for rotation in [90, 270] {
        let input = input(rotation, true);
        let parsed = parse_pdf_bytes("owned-rotated-visible.pdf", &input).unwrap();
        assert_eq!(parsed.image_elements.len(), 1);
        assert!(
            image_requires_ocr(&parsed, &parsed.image_elements[0]),
            "rotation={rotation}"
        );
        assert!(
            parsed
                .quality
                .issues
                .contains(&QualityIssue::OcrRequired { page: 0 })
        );
    }
}

/// 回転後の下端外の画像を元の高さで可視とみなさず、本文だけのstrictを拒否しない。
#[test]
fn image_below_rotated_display_does_not_require_ocr() {
    for rotation in [90, 270] {
        let input = input(rotation, false);
        let parsed = parse_pdf_bytes("owned-rotated-outside.pdf", &input).unwrap();
        assert_eq!(parsed.image_elements.len(), 1);
        assert!(
            !image_requires_ocr(&parsed, &parsed.image_elements[0]),
            "rotation={rotation}"
        );
        assert!(
            !parsed
                .quality
                .issues
                .contains(&QualityIssue::OcrRequired { page: 0 })
        );
    }
}

/// strictの実際の返り値も、回転した表示内外の画像に対応することを検査する。
#[test]
fn strict_result_follows_rotated_image_visibility() {
    for rotation in [90, 270] {
        let options = ParseOptions {
            strict: true,
            ..Default::default()
        };
        assert!(
            matches!(
                parse_pdf_bytes_with_options(
                    "owned-rotated-visible.pdf",
                    &input(rotation, true),
                    &options
                ),
                Err(ParseError::QualityRejected { .. })
            ),
            "visible image rotation={rotation}"
        );
        assert!(
            parse_pdf_bytes_with_options(
                "owned-rotated-outside.pdf",
                &input(rotation, false),
                &options
            )
            .is_ok(),
            "outside image rotation={rotation}"
        );
    }
}

/// 正式ページの文字も、回転で移った右端帯を可視領域として使う。
#[test]
fn rotated_source_text_uses_the_display_frame() {
    let input = fixture::rgb_with_page_geometry(
        200,
        300,
        270,
        b"BT /F1 12 Tf 10 50 Td (retained text) Tj ET",
    );
    let parsed = parse_pdf_bytes("owned-rotated-text.pdf", &input).unwrap();
    assert!(!parsed.text_elements.is_empty());
    assert!(
        parsed
            .text_elements
            .iter()
            .all(|text| text_is_visible(&parsed, text))
    );
}

/// file/native経路でも同じ表示内外を使い、入力byteを変更せずstrictを判定する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_strict_result_uses_the_rotated_display_frame() {
    use std::io::Write;
    for rotation in [90, 270] {
        for visible in [true, false] {
            let original = input(rotation, visible);
            let path = paths::temp_file_path("jade-rotated-visibility").with_extension("pdf");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            file.write_all(&original).unwrap();
            drop(file);
            let native = jade_pdf2docx_parser::parse_pdf(&path);
            let strict = jade_pdf2docx_parser::parse_pdf_with_options(
                &path,
                &ParseOptions {
                    strict: true,
                    ..Default::default()
                },
            );
            let preserved = std::fs::read(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            assert_eq!(preserved, original);
            let native = native.unwrap();
            assert_eq!(
                native
                    .quality
                    .issues
                    .contains(&QualityIssue::OcrRequired { page: 0 }),
                visible,
                "rotation={rotation} visible={visible}: {:?}",
                native.quality
            );
            if visible {
                assert!(matches!(strict, Err(ParseError::QualityRejected { .. })));
            } else {
                assert!(strict.is_ok(), "{strict:?}");
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 直角回転で表示座標が変わっても、元ページと画像の交差およびstrict結果を保つ。
    #[test]
    fn visibility_and_strict_are_covariant_under_page_rotation(
        turn in 0i32..4, width in 80u16..500, height in 80u16..500,
        x in -100i16..500, y in -100i16..500,
    ) {
        let content=format!("q 20 0 0 10 {x} {y} cm /Im Do Q BT /F1 12 Tf 10 50 Td (retained text) Tj ET");
        let input=fixture::rgb_with_page_geometry(u32::from(width),u32::from(height),turn*90,content.as_bytes());
        let expected=i32::from(x).max(0)<(i32::from(x)+20).min(i32::from(width))&&i32::from(y).max(0)<(i32::from(y)+10).min(i32::from(height));
        let parsed=parse_pdf_bytes("owned-rotated-property.pdf",&input).unwrap();
        prop_assert_eq!(parsed.image_elements.len(),1);
        prop_assert_eq!(image_requires_ocr(&parsed,&parsed.image_elements[0]),expected);
        prop_assert_eq!(parsed.quality.issues.contains(&QualityIssue::OcrRequired{page:0}),expected);
        let strict=parse_pdf_bytes_with_options("owned-rotated-property.pdf",&input,&ParseOptions{strict:true,..Default::default()});
        prop_assert_eq!(matches!(strict,Err(ParseError::QualityRejected{..})),expected,"strict rejection follows the original page intersection");
        if !expected { prop_assert!(strict.is_ok()); }
    }
}
