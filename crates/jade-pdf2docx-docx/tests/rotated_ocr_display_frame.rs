//! 実際のparserが正規化した表示座標を、公開OCR APIの切り出しと照合まで保持する。
#![cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_docx::{
    MinimumPdfToDocxRequest, OcrPageContent, PageOcrLimits, ParseOptions, PositionedOcrLine,
    convert_minimum_pdf_to_docx_with_bounded_page_ocr_content,
    convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
};
use std::{fs, io::Read, path::Path};

/// 所有する合成原本だけを一時ディレクトリへ保存し、既存出力のないrequestを作る。
fn request(directory: &Path, bytes: &[u8]) -> MinimumPdfToDocxRequest {
    let input = directory.join("owned.pdf");
    fs::write(&input, bytes).unwrap();
    MinimumPdfToDocxRequest {
        input_pdf_path: input,
        output_docx_path: directory.join("result.docx"),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    }
}

/// 成功したDOCXの本文XMLを読み、本文やpage frameを検査する。
fn body(path: &Path) -> String {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 表示右端の画像を元ページ幅で切らず、回転済みrasterの正しい領域をcallbackへ渡す。
#[test]
fn rotated_region_callback_receives_the_normalized_display_rectangle() {
    for (rotation, matrix, expected) in [
        (90, "20 0 0 10 10 250", [500, 20, 520, 60]),
        (270, "20 0 0 10 10 20", [540, 340, 560, 380]),
    ] {
        let content =
            format!("q {matrix} cm /Im Do Q BT /F1 12 Tf 10 50 Td (Native heading) Tj ET");
        let original = fixture::rgb_with_page_geometry(200, 300, rotation, content.as_bytes());
        let directory = tempfile::tempdir().unwrap();
        let request = request(directory.path(), &original);
        let mut called = 0;
        let report = convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
            &request,
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
            &PageOcrLimits {
                dpi: 144,
                ..Default::default()
            },
            |raster| {
                called += 1;
                assert_eq!((raster.width_px, raster.height_px), (600, 400));
                assert_eq!(raster.region, Some(expected), "rotation={rotation}");
                Ok(OcrPageContent::PositionedText(vec![PositionedOcrLine {
                    text: "Scanned".into(),
                    bounds: expected,
                }]))
            },
        )
        .unwrap();
        assert_eq!(called, 1);
        assert!(report.quality.issues.is_empty());
        let xml = body(&request.output_docx_path);
        assert_eq!(xml.matches(">Scanned</w:t>").count(), 1);
        assert!(xml.contains("Native heading"));
        assert_eq!(fs::read(&request.input_pdf_path).unwrap(), original);
    }
}

/// 認識画像内のnative行と同じ表示frameへ戻し、一致した本文を二重化しない。
#[test]
fn rotated_positioned_ocr_reconciles_the_normalized_native_line() {
    let content = b"q 0 60 -30 0 60 230 cm /Im Do Q q 0 1 -1 0 50 240 cm BT /F1 12 Tf 0 0 Td (Match) Tj ET Q BT /F1 12 Tf 10 50 Td (Native heading) Tj ET";
    let original = fixture::rgb_with_page_geometry(200, 300, 90, content);
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path(), &original);
    let report = convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
        &PageOcrLimits {
            dpi: 72,
            ..Default::default()
        },
        |raster| {
            assert_eq!(raster.region, Some([230, 30, 290, 60]));
            Ok(OcrPageContent::PositionedText(vec![PositionedOcrLine {
                text: "Match".into(),
                bounds: [238, 36, 276, 56],
            }]))
        },
    )
    .unwrap();
    assert!(report.quality.issues.is_empty());
    let xml = body(&request.output_docx_path);
    assert_eq!(xml.matches("Match").count(), 1, "{xml}");
    assert!(xml.contains("Native heading"));
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), original);
}

/// 座標付き全ページOCRのpage frameとcaller順序は、各直角回転でも変えない。
#[test]
fn whole_page_ocr_preserves_rendered_frames_through_all_cardinal_rotations() {
    for rotation in [0, 90, 180, 270] {
        let original =
            fixture::rgb_with_page_geometry(200, 300, rotation, b"q 60 0 0 30 10 10 cm /Im Do Q");
        let directory = tempfile::tempdir().unwrap();
        let request = request(directory.path(), &original);
        let report = convert_minimum_pdf_to_docx_with_bounded_page_ocr_content(
            &request,
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
            &PageOcrLimits {
                dpi: 72,
                ..Default::default()
            },
            |raster| {
                assert_eq!(raster.region, None);
                Ok(OcrPageContent::PositionedText(vec![
                    PositionedOcrLine {
                        text: "First".into(),
                        bounds: [10, 20, 30, 40],
                    },
                    PositionedOcrLine {
                        text: "Second".into(),
                        bounds: [50, 20, 70, 40],
                    },
                ]))
            },
        )
        .unwrap();
        assert_eq!(report.source_text_preview, "First\nSecond");
        let xml = body(&request.output_docx_path);
        let mut cursor = 0;
        for (text, expected_x) in [("First", 200), ("Second", 1000)] {
            let at = cursor + xml[cursor..].find(text).unwrap();
            let start = xml[..at]
                .rfind("<w:p ")
                .or_else(|| xml[..at].rfind("<w:p>"))
                .unwrap();
            let paragraph = &xml[start..at];
            for value in [
                format!("w:x=\"{expected_x}\""),
                "w:y=\"400\"".into(),
                "w:w=\"400\"".into(),
                "w:h=\"400\"".into(),
            ] {
                assert!(
                    paragraph.contains(&value),
                    "rotation={rotation} {paragraph}"
                );
            }
            cursor = at + text.len();
        }
        assert_eq!(fs::read(&request.input_pdf_path).unwrap(), original);
    }
}
