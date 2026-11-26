//! 回転後の表示矩形で可視範囲とstrictのOCR確認要求を判定する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{
    ParseError, ParseOptions, QualityIssue, image_requires_ocr, parse_pdf_bytes,
    parse_pdf_bytes_with_options, text_is_visible,
};

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
