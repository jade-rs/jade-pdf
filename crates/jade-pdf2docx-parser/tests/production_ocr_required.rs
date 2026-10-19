use jade_pdf2docx_parser::{
    ParseError, ParseOptions, QualityIssue, parse_pdf_bytes, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Write;

/// 描画画像と任意の文字レイヤを持つページを生成する。
fn image_pages(contents: &[&str]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    for (index, content) in contents.iter().enumerate() {
        let page = index * 2 + 1;
        let stream = page + 1;
        pdf.extend_from_slice(format!("{page} 0 obj << /Type /Page /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 100 0 R >> /Font << /F1 101 0 R >> >> /Contents {stream} 0 R >> endobj\n{stream} 0 obj << /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len()).as_bytes());
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[0, 0, 0]).unwrap();
    let pixels = encoder.finish().unwrap();
    pdf.extend_from_slice(format!("100 0 obj << /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode /Length {} >>\nstream\n", pixels.len()).as_bytes());
    pdf.extend_from_slice(&pixels);
    pdf.extend_from_slice(b"\nendstream\nendobj\n101 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n");
    pdf
}

/// 一部の行だけに文字レイヤがあっても、残りの画像本文を確認するまで診断を保持する。
#[test]
fn partial_text_layer_cannot_prove_image_completeness() {
    let pdf = image_pages(&[
        "q 100 0 0 100 20 20 cm /Im1 Do Q BT /F1 12 Tf 20 65 Td (Scanned above) Tj ET",
    ]);
    let parsed = parse_pdf_bytes("partial-row.pdf", &pdf).unwrap();
    assert!(!parsed.text_elements.is_empty());
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "partial-row.pdf",
            &pdf,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}

/// 画像だけのページは同一画像の再描画にかかわらず一度だけ OCR 必要を報告する。
#[test]
fn image_only_page_reports_once_and_strict_rejects() {
    let pdf = image_pages(&["q 200 0 0 200 0 0 cm /Im1 Do Q q 20 0 0 20 10 10 cm /Im1 Do Q"]);
    let parsed = parse_pdf_bytes("scan.pdf", &pdf).unwrap();
    assert_eq!(parsed.image_elements.len(), 2);
    assert_eq!(parsed.quality.issues.len(), 1, "missing OCR diagnosis");
    assert_eq!(
        parsed.quality.issues,
        vec![QualityIssue::OcrRequired { page: 0 }]
    );
    assert!(parsed.quality.issues[0].to_string().contains("page 1"));
    let strict = ParseOptions {
        strict: true,
        ..Default::default()
    };
    match parse_pdf_bytes_with_options("scan.pdf", &pdf, &strict) {
        Err(ParseError::QualityRejected { report, .. }) => assert_eq!(report, parsed.quality),
        other => panic!("expected OCR quality rejection: {other:?}"),
    }
}

/// 混在PDFでは文字レイヤの有無によらず可視画像ページを報告する。
#[test]
fn mixed_pages_report_all_visible_image_pages() {
    let pdf = image_pages(&[
        "q 200 0 0 200 0 0 cm /Im1 Do Q BT /F1 12 Tf 10 100 Td (Readable text layer) Tj ET",
        "q 200 0 0 200 0 0 cm /Im1 Do Q BT /F1 12 Tf 10 100 Td (   ) Tj ET",
        "",
    ]);
    let parsed = parse_pdf_bytes("mixed.pdf", &pdf).unwrap();
    assert_eq!(parsed.quality.issues.len(), 2);
    assert_eq!(
        parsed.quality.issues,
        vec![
            QualityIssue::OcrRequired { page: 0 },
            QualityIssue::OcrRequired { page: 1 }
        ]
    );
    assert!(parsed.quality.issues[1].to_string().contains("page 2"));
}

/// 空ページ・未描画 resource・可視領域外・ゼロ面積画像は OCR 必要と誤診しない。
#[test]
fn pages_without_visible_images_pass_strict() {
    let pdf = image_pages(&[
        "",
        "q 20 0 0 20 300 300 cm /Im1 Do Q",
        "q 0 0 0 0 10 10 cm /Im1 Do Q",
    ]);
    let parsed = parse_pdf_bytes_with_options(
        "blank.pdf",
        &pdf,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(parsed.quality.issues.is_empty());
}

/// 座標付き文字レイヤだけでは画像本文の完全性を保証しない。
#[test]
fn image_page_with_text_layer_requires_verification() {
    let pdf = image_pages(&[
        "q 200 0 0 200 0 0 cm /Im1 Do Q BT /F1 12 Tf 10 100 Td (Recognized scan text) Tj ET",
    ]);
    let parsed = parse_pdf_bytes("ocr.pdf", &pdf).unwrap();
    assert!(!parsed.text_elements.is_empty());
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
}

/// 画像から離れたnative見出しは画像本文のOCR要求を解消しない。
#[test]
fn separate_native_heading_does_not_hide_scanned_body() {
    let pdf = image_pages(&[
        "q 100 0 0 60 20 20 cm /Im1 Do Q BT /F1 12 Tf 10 160 Td (Native heading) Tj ET",
    ]);
    let parsed = parse_pdf_bytes("partial-layer.pdf", &pdf).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
}

/// ページ外・ゼロ面積の文字は画像本文のOCR要求を解消しない。
#[test]
fn invisible_text_layer_does_not_suppress_image_ocr() {
    for text in [
        "BT /F1 12 Tf 300 100 Td (Outside page) Tj ET",
        "BT /F1 12 Tf 10 300 Td (Outside page) Tj ET",
        "BT /F1 0 Tf 10 100 Td (Zero size) Tj ET",
    ] {
        let content = format!("q 200 0 0 200 0 0 cm /Im1 Do Q {text}");
        let pdf = image_pages(&[&content]);
        let parsed = parse_pdf_bytes("invisible-layer.pdf", &pdf).unwrap();
        assert!(
            !parsed.text_elements.is_empty(),
            "fixture must contain text"
        );
        assert_eq!(
            parsed.quality.issues,
            vec![QualityIssue::OcrRequired { page: 0 }],
            "invisible text must not hide OCR requirement: {text}"
        );
        assert!(matches!(
            parse_pdf_bytes_with_options(
                "invisible-layer.pdf",
                &pdf,
                &ParseOptions {
                    strict: true,
                    ..Default::default()
                }
            ),
            Err(ParseError::QualityRejected { .. })
        ));
    }
}

proptest! {
    /// 任意の文字位置・サイズでも可視画像本文の確認要求を保持する。
    #[test]
    fn text_layer_cannot_prove_image_completeness(x in -250i32..251, y in -250i32..251, font_size in 0u32..30) {
        let text = format!("BT /F1 {font_size} Tf {x} {y} Td (Layer) Tj ET");
        let content = format!("q 200 0 0 200 0 0 cm /Im1 Do Q {text}");
        let parsed = parse_pdf_bytes("text-bounds.pdf", &image_pages(&[&content])).unwrap();
        prop_assert_eq!(parsed.quality.issues, vec![QualityIssue::OcrRequired { page: 0 }]);
    }

    /// 平行移動・反転した画像でも可視領域との正面積の交差だけを診断する。
    #[test]
    fn ocr_diagnosis_matches_visible_intersection(x in -250i32..251, y in -250i32..251, width in -200i32..201, height in -200i32..201) {
        let content = format!("q {width} 0 0 {height} {x} {y} cm /Im1 Do Q");
        let pdf = image_pages(&[&content]);
        let parsed = parse_pdf_bytes("bounds.pdf", &pdf).unwrap();
        let visible = x.min(x + width).max(0) < x.max(x + width).min(200)
            && y.min(y + height).max(0) < y.max(y + height).min(200);
        let expected = if visible { vec![QualityIssue::OcrRequired { page: 0 }] } else { Vec::new() };
        prop_assert_eq!(parsed.quality.issues, expected);
    }
}

/// 一つの画像だけに文字が重なっても、別の画像本文を認識済みと推測しない。
#[test]
fn text_on_one_image_does_not_cover_a_separate_image() {
    let pdf = image_pages(&[
        "q 60 0 0 60 20 20 cm /Im1 Do Q q 60 0 0 60 120 20 cm /Im1 Do Q BT /F1 12 Tf 25 40 Td (Layer) Tj ET",
    ]);
    let parsed = parse_pdf_bytes("two-images.pdf", &pdf).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
}
