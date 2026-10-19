use std::{
    fs,
    io::{Read, Write},
};

use jade_pdf2docx_docx::{
    MinimumPdfToDocxRequest, ParseOptions, convert_minimum_pdf_to_docx_with_page_ocr,
};

/// 分割描画されたnative行の空白・文字色を残し、画像だけの行を補う。
#[test]
fn fragmented_native_row_preserves_spacing_and_style() {
    verify_fragmented_native_row("(Scanned ) Tj 1 0 0 rg (above) Tj", &["Scanned ", "above"]);
}

/// 空白だけを独立して描画するPDFでも、本文の単語境界を保持する。
#[test]
fn standalone_native_space_preserves_word_boundary() {
    verify_fragmented_native_row(
        "(Scanned) Tj 0 0 1 rg ( ) Tj 1 0 0 rg (above) Tj",
        &["Scanned", " ", "above"],
    );
}

/// 完全透明な別本文を照合へ渡さず、可視native行とOCRの重複を除く。
#[test]
fn transparent_native_text_does_not_block_ocr_reconciliation() {
    verify_fragmented_native_row(
        "q /Zero gs (Hidden mismatch) Tj Q 1 0 0 1 20 65 Tm (Scanned ) Tj 1 0 0 rg (above) Tj",
        &["Scanned ", "above"],
    );
}

/// clipの外側の別本文を照合へ渡さず、native行とOCRを統合する。
#[test]
fn clipped_native_text_does_not_block_ocr_reconciliation() {
    verify_fragmented_native_row(
        "ET q 0 0 1 1 re W n BT /F1 12 Tf 20 65 Td (Hidden mismatch) Tj ET Q BT /F1 12 Tf 20 65 Td (Scanned ) Tj 1 0 0 rg (above) Tj",
        &["Scanned ", "above"],
    );
}

/// 一つのTjのclip外prefixを除き、残った行をOCRと重複させずに保持する。
#[test]
fn partially_clipped_native_text_does_not_block_ocr_reconciliation() {
    verify_fragmented_native_row(
        "ET q 20 60 200 20 re W n BT /F1 12 Tf 3.992 65 Td (XXScanned ) Tj 1 0 0 rg (above) Tj ET Q BT",
        &["Scanned ", "above"],
    );
}

/// native重複排除が曖昧な部分clipはOCR前に拒否し、既存DOCXと原本を保持する。
#[test]
fn overlapping_partial_clip_preserves_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    let original = mixed_pdf_with_image_text(
        "q 20 60 200 20 re W n BT /F1 12 Tf 3.992 65 Td (XXScanned) Tj ET Q BT /F1 12 Tf 3.992 65 Td (XX) Tj ET",
    );
    fs::write(&request.input_pdf_path, &original).unwrap();
    let previous = b"previous complete DOCX";
    fs::write(&request.output_docx_path, previous).unwrap();
    for strict in [false, true] {
        let result = convert_minimum_pdf_to_docx_with_page_ocr(
            &request,
            &ParseOptions {
                strict,
                ..Default::default()
            },
            |_| panic!("ambiguous native text must be rejected before OCR"),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("overlapping partially clipped text")
        );
        assert_eq!(fs::read(&request.output_docx_path).unwrap(), previous);
        assert_eq!(fs::read(&request.input_pdf_path).unwrap(), original);
    }
}

/// native断片の配置・本文・書式と、重複しないOCR補完を検証する。
fn verify_fragmented_native_row(operators: &str, expected_fragments: &[&str]) {
    use jade_pdf2docx_docx::{
        OcrPageContent, PageOcrLimits, PositionedOcrLine,
        convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    };
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    let original = mixed_pdf_with_image_text(&format!("BT /F1 12 Tf 20 65 Td {operators} ET"));
    fs::write(&request.input_pdf_path, &original).unwrap();
    let parsed = jade_pdf2docx_parser::parse_pdf_bytes("split.pdf", &original).unwrap();
    let fragments = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == 1)
        .collect::<Vec<_>>();
    assert_eq!(
        fragments
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>(),
        expected_fragments
    );
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
        |_| {
            Ok(OcrPageContent::PositionedText(vec![
                PositionedOcrLine {
                    text: "Scanned above".into(),
                    bounds: [20, 123, 100, 135],
                },
                PositionedOcrLine {
                    text: "Scanned below".into(),
                    bounds: [20, 165, 100, 175],
                },
            ]))
        },
    )
    .unwrap();
    assert_eq!(
        report.source_text_preview,
        "Native before\nScanned above\nScanned below\nNative after"
    );
    assert!(report.quality.issues.is_empty());
    let mut archive =
        zip::ZipArchive::new(fs::File::open(&request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(xml.matches("Scanned").count(), 2, "{xml}");
    assert_eq!(xml.matches("above").count(), 1, "{xml}");
    let body = xml
        .split("</w:t>")
        .filter_map(|part| part.rsplit_once('>').map(|(_, text)| text))
        .collect::<String>();
    assert!(body.contains("Scanned above"), "{xml}");
    assert!(xml.contains("w:color w:val=\"FF0000\""), "{xml}");
    if expected_fragments.len() == 3 {
        assert!(xml.contains("w:color w:val=\"0000FF\""), "{xml}");
    } else {
        assert!(xml.contains(">Scanned </w:t>"), "{xml}");
    }
    assert_eq!(xml.matches("w:type=\"page\"").count(), 2);
    assert!(!xml.contains("<w:drawing"));
    assert!(!xml.contains("Hidden mismatch"));
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), original);
}

/// 部分文字レイヤの一致行を二重化せず、文字レイヤのない画像行を補う。
#[test]
fn partial_text_layer_keeps_native_and_missing_ocr_rows_once() {
    use jade_pdf2docx_docx::{
        OcrPageContent, PageOcrLimits, PositionedOcrLine,
        convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    };
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(
        &request.input_pdf_path,
        mixed_pdf_with_image_text("BT /F1 12 Tf 20 65 Td (Scanned above) Tj ET"),
    )
    .unwrap();
    let mut calls = 0;
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
        |_| {
            calls += 1;
            Ok(OcrPageContent::PositionedText(vec![
                PositionedOcrLine {
                    text: "Scanned above".into(),
                    bounds: [20, 123, 100, 135],
                },
                PositionedOcrLine {
                    text: "Scanned below".into(),
                    bounds: [20, 165, 100, 175],
                },
            ]))
        },
    )
    .unwrap();
    assert_eq!(calls, 1, "partial text layer must not suppress OCR");
    assert_eq!(
        report.source_text_preview,
        "Native before\nScanned above\nScanned below\nNative after"
    );
    let mut archive =
        zip::ZipArchive::new(fs::File::open(&request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    for text in ["Scanned above", "Scanned below"] {
        assert_eq!(xml.matches(text).count(), 1, "{xml}");
    }
    assert!(!xml.contains("<w:drawing"));
    assert!(report.quality.issues.is_empty());
}

/// 重なる本文の食い違い・座標欠落・白紙判定では既存出力を保持する。
#[test]
fn ambiguous_partial_text_layer_preserves_output() {
    use jade_pdf2docx_docx::{
        OcrPageContent, PageOcrLimits, PositionedOcrLine, VerifiedBlankPage,
        convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    };
    for content in [
        OcrPageContent::PositionedText(vec![PositionedOcrLine {
            text: "Different text".into(),
            bounds: [20, 123, 100, 135],
        }]),
        // 同じ本文でも別行・僅かな交差・複数候補は一致とみなさない。
        OcrPageContent::PositionedText(vec![PositionedOcrLine {
            text: "Scanned above".into(),
            bounds: [20, 165, 100, 175],
        }]),
        OcrPageContent::PositionedText(vec![PositionedOcrLine {
            text: "Scanned above".into(),
            bounds: [99, 123, 119, 135],
        }]),
        OcrPageContent::PositionedText(vec![
            PositionedOcrLine {
                text: "Scanned above".into(),
                bounds: [20, 123, 100, 135],
            },
            PositionedOcrLine {
                text: "Scanned above".into(),
                bounds: [20, 123, 100, 135],
            },
        ]),
        OcrPageContent::PositionedText(vec![PositionedOcrLine {
            text: "Scannedabove".into(),
            bounds: [20, 123, 100, 135],
        }]),
        OcrPageContent::Text(vec!["Scanned above".into()]),
        OcrPageContent::Blank(VerifiedBlankPage::from_rgb8(&[255, 255, 255], 1, 1).unwrap()),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let request = request(directory.path());
        fs::write(
            &request.input_pdf_path,
            mixed_pdf_with_image_text("BT /F1 12 Tf 20 65 Td (Scanned above) Tj ET"),
        )
        .unwrap();
        fs::write(&request.output_docx_path, b"keep").unwrap();
        let result = convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
            &request,
            &ParseOptions::default(),
            &PageOcrLimits {
                dpi: 72,
                ..Default::default()
            },
            |_| Ok(content.clone()),
        );
        assert!(result.is_err(), "ambiguous overlap must fail");
        assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
    }
}

/// 画像の横にあるnative本文と認識行を、実際の行座標の順序で配置する。
#[test]
fn positioned_scan_lines_interleave_with_native_text() {
    use jade_pdf2docx_docx::{
        PageOcrLimits, convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    };
    use jade_pdf2docx_ocr::{OcrPageContent, PositionedOcrLine};
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(
        &request.input_pdf_path,
        mixed_pdf_with_image_text("BT /F1 12 Tf 130 50 Td (Native middle) Tj ET"),
    )
    .unwrap();
    let report = convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions::default(),
        &PageOcrLimits {
            dpi: 72,
            ..Default::default()
        },
        |_| {
            Ok(OcrPageContent::PositionedText(vec![
                PositionedOcrLine {
                    text: "Scanned above".into(),
                    bounds: [20, 125, 100, 135],
                },
                PositionedOcrLine {
                    text: "Scanned below".into(),
                    bounds: [20, 165, 100, 175],
                },
            ]))
        },
    )
    .unwrap();
    let expected = "Native before\nScanned above\nNative middle\nScanned below\nNative after";
    assert_eq!(report.source_text_preview, expected);
    let mut archive =
        zip::ZipArchive::new(fs::File::open(&request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let offsets: Vec<_> = expected
        .lines()
        .map(|text| xml.find(text).unwrap())
        .collect();
    assert!(offsets.windows(2).all(|pair| pair[0] < pair[1]), "{xml}");
}

/// 外部callbackの不正な行座標は、出力を変更せず拒否する。
#[test]
fn invalid_positioned_lines_preserve_existing_output() {
    use jade_pdf2docx_docx::{
        OcrPageContent, PageOcrLimits, PositionedOcrLine,
        convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    };
    for bounds in [
        [20, 125, 20, 135],
        [100, 125, 20, 135],
        [19, 125, 100, 135],
        [20, 79, 100, 135],
        [20, 125, 121, 135],
        [20, 125, 100, 181],
        [0, 0, usize::MAX, usize::MAX],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let request = request(directory.path());
        fs::write(
            &request.input_pdf_path,
            mixed_pdf_with_image_text("BT /F1 12 Tf 130 50 Td (Native middle) Tj ET"),
        )
        .unwrap();
        fs::write(&request.output_docx_path, b"keep").unwrap();
        let result = convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
            &request,
            &ParseOptions::default(),
            &PageOcrLimits {
                dpi: 72,
                ..Default::default()
            },
            |_| {
                Ok(OcrPageContent::PositionedText(vec![PositionedOcrLine {
                    text: "body".into(),
                    bounds,
                }]))
            },
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid OCR line bounds"),
            "{bounds:?}"
        );
        assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
    }
}

/// 座標付き本文も全領域でbyte・行数の残量を共有し、上限超過では部分保存しない。
#[test]
fn positioned_lines_obey_text_limits() {
    use jade_pdf2docx_docx::{
        OcrPageContent, OcrTextLimits, PageOcrLimits, PositionedOcrLine,
        convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    };
    for (max_bytes, max_lines, success) in [(5, 1, true), (4, 1, false), (5, 0, false)] {
        let directory = tempfile::tempdir().unwrap();
        let request = request(directory.path());
        fs::write(
            &request.input_pdf_path,
            mixed_pdf_with_image_text("BT /F1 12 Tf 130 50 Td (Native middle) Tj ET"),
        )
        .unwrap();
        fs::write(&request.output_docx_path, b"keep").unwrap();
        let result = convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
            &request,
            &ParseOptions::default(),
            &PageOcrLimits {
                dpi: 72,
                text: OcrTextLimits {
                    max_bytes,
                    max_lines,
                },
                ..Default::default()
            },
            |_| {
                Ok(OcrPageContent::PositionedText(vec![PositionedOcrLine {
                    text: "body".into(),
                    bounds: [20, 125, 100, 135],
                }]))
            },
        );
        assert_eq!(result.is_ok(), success, "{result:?}");
        if !success {
            assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
        }
    }
}

/// 検証済みの白紙は本文予算ゼロでも保存でき、前後の本文とページ境界を保持する。
#[test]
fn verified_blank_page_keeps_boundaries_without_consuming_text_budget() {
    use jade_pdf2docx_docx::{
        OcrPageContent, OcrTextLimits, PageOcrLimits, VerifiedBlankPage,
        convert_minimum_pdf_to_docx_with_bounded_page_ocr_content,
    };
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    let mut visited = Vec::new();
    let report = convert_minimum_pdf_to_docx_with_bounded_page_ocr_content(
        &request,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
        &PageOcrLimits {
            text: OcrTextLimits {
                max_bytes: 0,
                max_lines: 0,
            },
            ..Default::default()
        },
        |raster| {
            visited.push(raster.page);
            Ok(OcrPageContent::Blank(
                VerifiedBlankPage::from_rgb8(&[255; 3], 1, 1).unwrap(),
            ))
        },
    )
    .unwrap();
    assert_eq!(visited, [1]);
    assert!(report.quality.issues.is_empty());
    assert_eq!(report.source_text_preview, "Native before\nNative after");
    let mut archive =
        zip::ZipArchive::new(fs::File::open(&request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(xml.matches("w:type=\"page\"").count(), 2, "{xml}");
    assert!(!xml.contains("<w:drawing"));
    assert!(!xml.contains("no extractable content"));
}

/// 本文の空結果は白紙証明と異なり、保存前に拒否して既存出力を保持する。
#[test]
fn page_content_rejects_unverified_empty_text() {
    use jade_pdf2docx_docx::{
        OcrPageContent, PageOcrLimits, convert_minimum_pdf_to_docx_with_bounded_page_ocr_content,
    };
    for lines in [vec![], vec![" ".into()]] {
        let directory = tempfile::tempdir().unwrap();
        let request = request(directory.path());
        fs::write(&request.output_docx_path, b"keep").unwrap();
        let result = convert_minimum_pdf_to_docx_with_bounded_page_ocr_content(
            &request,
            &ParseOptions::default(),
            &PageOcrLimits::default(),
            |_| Ok(OcrPageContent::Text(lines.clone())),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("empty recognized text")
        );
        assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
    }
}

/// 不可視文字のある画像ページも認識し、ページ順と可視native本文を保持する。
#[test]
fn image_with_outside_text_is_recognized() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(
        &request.input_pdf_path,
        mixed_pdf_with_image_text("BT /F1 12 Tf 300 100 Td (Outside page) Tj ET"),
    )
    .unwrap();
    let mut visited = Vec::new();
    let report = convert_minimum_pdf_to_docx_with_page_ocr(
        &request,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
        |page| {
            visited.push(page);
            Ok(vec!["Scanned text".into()])
        },
    )
    .unwrap();
    assert_eq!(visited, [1]);
    assert!(report.quality.issues.is_empty());
    assert_eq!(
        report.source_text_preview,
        "Native before\nScanned text\nNative after"
    );
    let mut archive =
        zip::ZipArchive::new(fs::File::open(request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("Scanned text"));
    assert!(!xml.contains("Outside page"));
    assert_eq!(xml.matches("w:type=\"page\"").count(), 2);
}

/// 画像本文を認識してもnative見出しと脚注をそれぞれ一度だけ保持する。
#[test]
fn native_heading_and_footer_survive_scanned_body_recognition() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(&request.input_pdf_path, mixed_pdf_with_image_text(
        "BT /F1 12 Tf 20 160 Td (Native heading) Tj ET BT /F1 12 Tf 20 5 Td (Native footer) Tj ET"
    )).unwrap();
    let mut visited = Vec::new();
    let report = jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
        &jade_pdf2docx_docx::PageOcrLimits::default(),
        |raster| {
            visited.push(raster.page);
            assert!(raster.region.is_some());
            Ok(jade_pdf2docx_docx::OcrPageContent::Text(vec![
                "Scanned body".into(),
            ]))
        },
    )
    .unwrap();
    assert_eq!(visited, [1]);
    assert_eq!(
        report.source_text_preview,
        "Native before\nNative heading\nScanned body\nNative footer\nNative after"
    );
    let mut archive =
        zip::ZipArchive::new(fs::File::open(request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    for text in ["Native heading", "Scanned body", "Native footer"] {
        assert_eq!(xml.matches(text).count(), 1, "{xml}");
    }
    assert_eq!(xml.matches("w:type=\"page\"").count(), 2);
}

/// 本文が既定の累積byte上限を超えた場合、成功や部分保存にせず既存出力を保持する。
#[test]
fn oversized_recognized_text_preserves_existing_output() {
    use jade_pdf2docx_docx::{PageOcrLimits, convert_minimum_pdf_to_docx_with_bounded_page_ocr};
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let result = convert_minimum_pdf_to_docx_with_bounded_page_ocr(
        &request,
        &ParseOptions::default(),
        &PageOcrLimits::default(),
        |_| Ok(vec!["a".repeat(8 * 1024 * 1024)]),
    );
    let error = match result {
        Err(error) => error.to_string(),
        Ok(_) => panic!("oversized text must fail"),
    };
    assert!(error.contains("text budget"), "{error}");
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
}

/// 本文のbyte数と行数は全ページで共有し、枯渇した次ページの認識を開始しない。
#[test]
fn text_budget_is_cumulative_inclusive_and_preserves_output() {
    use jade_pdf2docx_docx::{
        OcrTextLimits, PageOcrLimits, convert_minimum_pdf_to_docx_with_bounded_page_ocr,
    };
    for (max_bytes, max_lines, succeeds, callbacks) in [
        (7, 2, false, 2),
        (8, 2, true, 2),
        (100, 1, false, 1),
        (0, 2, false, 0),
        (1, 2, false, 0),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let request = two_scan_request(directory.path());
        fs::write(&request.output_docx_path, b"keep").unwrap();
        let limits = PageOcrLimits {
            text: OcrTextLimits {
                max_bytes,
                max_lines,
            },
            ..Default::default()
        };
        let mut visited = Vec::new();
        let result = convert_minimum_pdf_to_docx_with_bounded_page_ocr(
            &request,
            &ParseOptions::default(),
            &limits,
            |raster| {
                visited.push(raster.page);
                let consumed = (visited.len() as u64 - 1) * 4;
                assert_eq!(raster.text.max_bytes, max_bytes - consumed);
                assert_eq!(raster.text.max_lines, max_lines - visited.len() as u64 + 1);
                Ok(vec!["日".into()])
            },
        );
        assert_eq!(visited.len(), callbacks);
        if succeeds {
            assert!(result.is_ok());
        } else {
            let error = match result {
                Err(error) => error.to_string(),
                Ok(_) => panic!("text capacity must reject"),
            };
            assert!(error.contains("text budget"), "{error}");
            assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
        }
    }
}

/// 最後のnativeページもスキャンへ変更し、xref offsetを維持する。
fn two_scan_request(directory: &std::path::Path) -> MinimumPdfToDocxRequest {
    let request = request(directory);
    let mut bytes = mixed_pdf();
    let old = b"BT /F1 12 Tf 20 100 Td (Native after) Tj ET";
    let offset = bytes
        .windows(old.len())
        .position(|part| part == old)
        .unwrap();
    let mut content = b"q 100 0 0 100 20 20 cm /Im1 Do Q".to_vec();
    content.resize(old.len(), b' ');
    bytes[offset..offset + old.len()].copy_from_slice(&content);
    fs::write(&request.input_pdf_path, bytes).unwrap();
    request
}

/// 画素上限を超えるOCRページはcallback開始前に拒否し、既存出力を保持する。
#[test]
fn page_ocr_pixel_budget_rejects_before_inference() {
    use jade_pdf2docx_docx::{PageOcrLimits, convert_minimum_pdf_to_docx_with_bounded_page_ocr};
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let limits = PageOcrLimits {
        dpi: 72,
        max_page_pixels: 40_000,
        max_total_pixels: 1_000_000,
        ..Default::default()
    };
    let result = convert_minimum_pdf_to_docx_with_bounded_page_ocr(
        &request,
        &ParseOptions::default(),
        &limits,
        |_| panic!("pixel budget must reject before inference"),
    );
    let error = result.unwrap_err().to_string();
    assert!(error.contains("pixel budget"), "{error}");
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
}

/// native本文とスキャンページが交互に現れる正式xref付きPDFを作る。
fn mixed_pdf() -> Vec<u8> {
    mixed_pdf_with_image_text("")
}

/// スキャンページへ任意の文字描画を加えた正式xref付きPDFを作る。
fn mixed_pdf_with_image_text(image_text: &str) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R] /Count 3 >>".to_owned(),
    ];
    for (index, content) in [
        "BT /F1 12 Tf 20 100 Td (Native before) Tj ET".to_owned(),
        format!("q 100 0 0 100 20 20 cm /Im1 Do Q {image_text}"),
        "BT /F1 12 Tf 20 100 Td (Native after) Tj ET".to_owned(),
    ]
    .iter()
    .enumerate()
    {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 9 0 R >> /ExtGState << /Zero << /ca 0 /CA 0 >> >> /XObject << /Im1 10 0 R >> >> /Contents {} 0 R >>", 4 + index * 2));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    let mut objects: Vec<Vec<u8>> = objects.into_iter().map(String::into_bytes).collect();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[0, 0, 0]).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut image = format!("<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", compressed.len()).into_bytes();
    image.extend(compressed);
    image.extend_from_slice(b"\nendstream");
    objects.push(image);
    let mut source = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        source.extend_from_slice(object);
        source.extend_from_slice(b"\nendobj\n");
    }
    let xref = source.len();
    source.extend_from_slice(b"xref\n0 11\n0000000000 65535 f \n");
    for offset in offsets {
        source.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    source.extend_from_slice(
        format!("trailer\n<< /Size 11 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    source
}

/// 一時入力と既存出力を持つ変換要求を作る。
fn request(directory: &std::path::Path) -> MinimumPdfToDocxRequest {
    let input = directory.join("mixed.pdf");
    fs::write(&input, mixed_pdf()).unwrap();
    MinimumPdfToDocxRequest {
        input_pdf_path: input,
        output_docx_path: directory.join("result.docx"),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    }
}

/// OCRページだけを置換し、前後のnative本文とページ境界・空白を保持する。
#[test]
fn page_ocr_preserves_native_pages_and_order() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    let mut visited = Vec::new();
    let report = convert_minimum_pdf_to_docx_with_page_ocr(
        &request,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
        |page| {
            visited.push(page);
            Ok(vec![" 111111 ".into(), "123123123".into()])
        },
    )
    .unwrap();
    assert_eq!(visited, [1]);
    assert!(report.quality.issues.is_empty());
    let mut preview = report.source_text_preview.as_str();
    for text in ["Native before", " 111111 ", "123123123", "Native after"] {
        let offset = preview.find(text).expect("preview must follow page order");
        preview = &preview[offset + text.len()..];
    }
    let mut archive =
        zip::ZipArchive::new(fs::File::open(&request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let mut after = xml.as_str();
    for text in ["Native before", " 111111 ", "123123123", "Native after"] {
        let offset = after
            .find(text)
            .unwrap_or_else(|| panic!("missing {text:?}: {xml}"));
        after = &after[offset + text.len()..];
    }
    assert_eq!(xml.matches("w:type=\"page\"").count(), 2, "{xml}");
    assert!(
        !xml.contains("<w:drawing>"),
        "OCR page must not duplicate its scan image"
    );
}

/// 推論失敗と空の認識結果では既存出力を保持する。
#[test]
fn page_ocr_failure_preserves_output() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    for result in [
        Err("model failed".into()),
        Ok(vec![]),
        Ok(vec![" \t".into()]),
    ] {
        fs::write(&request.output_docx_path, b"keep").unwrap();
        let error =
            convert_minimum_pdf_to_docx_with_page_ocr(&request, &ParseOptions::default(), |_| {
                result.clone()
            })
            .unwrap_err();
        assert!(error.to_string().contains("OCR"), "{error}");
        assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
    }
}

/// strictのOCR以外の品質拒否と入出力aliasは推論開始前に検出する。
#[test]
fn page_ocr_rejects_quality_and_alias_before_inference() {
    let directory = tempfile::tempdir().unwrap();
    let mut request = request(directory.path());
    let mut bytes = mixed_pdf();
    let offset = bytes
        .windows(11)
        .position(|part| part == b"FlateDecode")
        .unwrap();
    bytes[offset..offset + 11].copy_from_slice(b"OtherDecode");
    fs::write(&request.input_pdf_path, bytes).unwrap();
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let result = convert_minimum_pdf_to_docx_with_page_ocr(
        &request,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
        |_| panic!("must reject before inference"),
    );
    assert!(matches!(
        result,
        Err(jade_pdf2docx_docx::ConvertError::Parse(
            jade_pdf2docx_docx::ParseError::QualityRejected { .. }
        ))
    ));
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
    fs::write(&request.input_pdf_path, mixed_pdf()).unwrap();
    request.output_docx_path = request.input_pdf_path.clone();
    let result =
        convert_minimum_pdf_to_docx_with_page_ocr(&request, &ParseOptions::default(), |_| {
            panic!("must reject alias before inference")
        });
    assert!(matches!(
        result,
        Err(jade_pdf2docx_docx::ConvertError::OutputAliasesInput)
    ));
    assert_eq!(fs::read(request.input_pdf_path).unwrap(), mixed_pdf());
}

/// callback中に停止された結果をDOCXへ公開しない。
#[test]
fn page_ocr_checks_cancellation_after_inference() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let token = jade_pdf2docx_docx::ParseCancellation::new();
    let options = ParseOptions {
        cancellation: Some(token.clone()),
        ..Default::default()
    };
    let result = convert_minimum_pdf_to_docx_with_page_ocr(&request, &options, |_| {
        token.cancel();
        Ok(vec!["recognized".into()])
    });
    assert!(matches!(
        result,
        Err(jade_pdf2docx_docx::ConvertError::Parse(
            jade_pdf2docx_docx::ParseError::Cancelled
        ))
    ));
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
}

/// 後続ページの推論失敗でも先行ページの結果だけを公開しない。
#[test]
fn page_ocr_late_failure_preserves_output() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    let mut bytes = mixed_pdf();
    let old = b"BT /F1 12 Tf 20 100 Td (Native after) Tj ET";
    let offset = bytes
        .windows(old.len())
        .position(|part| part == old)
        .unwrap();
    let mut content = b"q 100 0 0 100 20 20 cm /Im1 Do Q".to_vec();
    content.resize(old.len(), b' ');
    bytes[offset..offset + old.len()].copy_from_slice(&content);
    fs::write(&request.input_pdf_path, bytes).unwrap();
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let mut visited = Vec::new();
    let result =
        convert_minimum_pdf_to_docx_with_page_ocr(&request, &ParseOptions::default(), |page| {
            visited.push(page);
            if page == 1 {
                Ok(vec!["first page".into()])
            } else {
                Err("second page failed".into())
            }
        });
    assert_eq!(visited, [1, 2]);
    assert!(matches!(
        result,
        Err(jade_pdf2docx_docx::ConvertError::Ocr(_))
    ));
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
}

/// 累積上限は最初のcallback前に検査し、上限と同じ値は受理する。
#[test]
fn cumulative_budget_is_inclusive_and_preflighted() {
    use jade_pdf2docx_docx::{PageOcrLimits, convert_minimum_pdf_to_docx_with_bounded_page_ocr};
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    let mut bytes = mixed_pdf();
    let old = b"BT /F1 12 Tf 20 100 Td (Native after) Tj ET";
    let offset = bytes
        .windows(old.len())
        .position(|part| part == old)
        .unwrap();
    let mut content = b"q 100 0 0 100 20 20 cm /Im1 Do Q".to_vec();
    content.resize(old.len(), b' ');
    bytes[offset..offset + old.len()].copy_from_slice(&content);
    fs::write(&request.input_pdf_path, bytes).unwrap();
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let mut limits = PageOcrLimits {
        dpi: 72,
        max_page_pixels: 40_401,
        max_total_pixels: 80_801,
        ..Default::default()
    };
    let error = convert_minimum_pdf_to_docx_with_bounded_page_ocr(
        &request,
        &ParseOptions::default(),
        &limits,
        |_| panic!("all pages must be checked before first callback"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("pixel budget"));
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
    limits.max_total_pixels += 1;
    let mut visited = Vec::new();
    convert_minimum_pdf_to_docx_with_bounded_page_ocr(
        &request,
        &ParseOptions::default(),
        &limits,
        |raster| {
            visited.push((raster.page, raster.width_px, raster.height_px));
            Ok(vec!["recognized".into()])
        },
    )
    .unwrap();
    assert_eq!(visited, [(1, 200, 200), (2, 200, 200)]);
}

/// 従来の全ページ置換callbackでnative本文を誤って消さず、認識前に拒否する。
#[test]
fn legacy_page_replacement_rejects_native_and_scan_before_callback() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(
        &request.input_pdf_path,
        mixed_pdf_with_image_text("BT /F1 12 Tf 20 160 Td (Heading) Tj ET"),
    )
    .unwrap();
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let result =
        convert_minimum_pdf_to_docx_with_page_ocr(&request, &ParseOptions::default(), |_| {
            panic!("must reject before recognition")
        });
    assert!(result.err().unwrap().to_string().contains("region OCR"));
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
}

/// 各領域でページを描画する費用を事前に累積し、超過時はcallbackも出力も変更しない。
#[test]
fn multiple_regions_reserve_all_renders_before_callback() {
    use jade_pdf2docx_docx::{
        PageOcrLimits, convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    };
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(
        &request.input_pdf_path,
        mixed_pdf_with_image_text(
            "q 40 0 0 60 140 20 cm /Im1 Do Q BT /F1 12 Tf 20 160 Td (Heading) Tj ET",
        ),
    )
    .unwrap();
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let limits = PageOcrLimits {
        dpi: 72,
        max_total_pixels: 2 * 201 * 201 - 1,
        ..Default::default()
    };
    let result = convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions::default(),
        &limits,
        |_| panic!("must reserve both regions"),
    );
    assert!(result.err().unwrap().to_string().contains("pixel budget"));
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
    let mut count = 0;
    let limits = PageOcrLimits {
        max_total_pixels: 2 * 201 * 201,
        ..limits
    };
    let report = convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions::default(),
        &limits,
        |raster| {
            count += 1;
            assert!(raster.region.is_some());
            Ok(jade_pdf2docx_docx::OcrPageContent::Text(vec![format!(
                "Body {count}"
            )]))
        },
    )
    .unwrap();
    assert_eq!(count, 2);
    for text in ["Heading", "Body 1", "Body 2"] {
        assert_eq!(report.source_text_preview.matches(text).count(), 1);
    }
}

/// 画像同士の合成を二重に認識せず、曖昧な領域は出力を保持して拒否する。
#[test]
fn overlapping_regions_are_rejected_before_recognition() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(
        &request.input_pdf_path,
        mixed_pdf_with_image_text(
            "q 40 0 0 60 60 20 cm /Im1 Do Q BT /F1 12 Tf 20 160 Td (Heading) Tj ET",
        ),
    )
    .unwrap();
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let result = jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions::default(),
        &jade_pdf2docx_docx::PageOcrLimits::default(),
        |_| panic!("must reject ambiguous composition"),
    );
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("overlapping images")
    );
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
}

/// 領域をまたぐ本文容量も共有し、後半の超過時にnative本文を含む部分成果を公開しない。
#[test]
fn region_text_budget_preserves_original_output_on_second_region_failure() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(
        &request.input_pdf_path,
        mixed_pdf_with_image_text(
            "q 40 0 0 60 140 20 cm /Im1 Do Q BT /F1 12 Tf 20 160 Td (Heading) Tj ET",
        ),
    )
    .unwrap();
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let limits = jade_pdf2docx_docx::PageOcrLimits {
        text: jade_pdf2docx_docx::OcrTextLimits {
            max_bytes: 5,
            max_lines: 1,
        },
        ..Default::default()
    };
    let mut count = 0;
    let result = jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions::default(),
        &limits,
        |raster| {
            assert_eq!(raster.text.max_lines, u64::from(count == 0));
            count += 1;
            Ok(jade_pdf2docx_docx::OcrPageContent::Text(vec![
                "body".into(),
            ]))
        },
    );
    assert_eq!(count, 2);
    assert!(result.err().unwrap().to_string().contains("text budget"));
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
}

/// 密な認識行をnative文字の行結合・ページ番号除去へ渡さず、空白と数値行を保持する。
#[test]
fn region_lines_preserve_whitespace_and_numeric_body_without_row_merging() {
    let directory = tempfile::tempdir().unwrap();
    let request = request(directory.path());
    fs::write(
        &request.input_pdf_path,
        mixed_pdf_with_image_text(
            "BT /F1 12 Tf 20 160 Td (Heading) Tj ET BT /F1 12 Tf 20 5 Td (Footer) Tj ET",
        ),
    )
    .unwrap();
    let mut lines: Vec<_> = (0..29).map(|index| format!("  body {index}  ")).collect();
    lines.push("7".into());
    let expected = format!(
        "Native before\nHeading\n{}\nFooter\nNative after",
        lines.join("\n")
    );
    let report = jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions::default(),
        &jade_pdf2docx_docx::PageOcrLimits::default(),
        |_| Ok(jade_pdf2docx_docx::OcrPageContent::Text(lines.clone())),
    )
    .unwrap();
    assert_eq!(report.source_text_preview, expected);
    let mut archive =
        zip::ZipArchive::new(fs::File::open(&request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    for line in lines {
        assert!(
            xml.contains(&format!(">{line}</w:t>")),
            "missing original line: {line}"
        );
    }
}
