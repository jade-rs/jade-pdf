use jade_pdf2docx_docx::{
    MinimumPdfToDocxRequest, ParseOptions, RenderedPageImage,
    convert_minimum_pdf_bytes_to_docx_bytes, convert_minimum_pdf_bytes_to_docx_bytes_with_options,
    convert_minimum_pdf_to_docx_with_options, convert_rendered_png_pages_to_docx_bytes,
};
use std::{
    fs,
    io::{Cursor, Read},
};

/// Word本文XMLを展開し、ページや画像の寸法を直接検証する。
fn document(bytes: Vec<u8>) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}
/// 指定tagの先頭属性を、XML writerの属性順によらず読む。
fn attribute(xml: &str, tag: &str, name: &str) -> u64 {
    let start = xml.find(&format!("<{tag} ")).unwrap();
    let body = &xml[start..start + xml[start..].find('>').unwrap()];
    let needle = format!("{name}=\"");
    let at = body.find(&needle).unwrap() + needle.len();
    body[at..].split('"').next().unwrap().parse().unwrap()
}
/// 原寸・文字サイズ・線分を指定した正式xref付きowned PDFを生成する。
fn pdf(width: u32, height: u32) -> Vec<u8> {
    let content = "BT /F1 20 Tf 100 100 Td (Owned wide page) Tj ET\n2 w 100 80 m 1800 80 l S";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    owned_objects(&objects)
}

/// 5個のowned objectへ正式なxrefを付ける。
fn owned_objects(objects: &[String; 5]) -> Vec<u8> {
    owned_byte_objects(&std::array::from_fn(|i| objects[i].as_bytes().to_vec()))
}

/// 圧縮streamを含む5objectへxrefを付け、元bytesを保持する。
fn owned_byte_objects(objects: &[Vec<u8>; 5]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 30in近いページでも、枠だけを切らず原寸の比率と本文・文字サイズを同時に縮める。
#[test]
fn wide_page_and_content_fit_together() {
    let source = pdf(2000, 1000);
    let xml = document(convert_minimum_pdf_bytes_to_docx_bytes("wide.pdf", &source).unwrap());
    assert_eq!(
        attribute(&xml, "w:pgSz", "w:w"),
        31680,
        "unsupported Word page width"
    );
    assert_eq!(attribute(&xml, "w:pgSz", "w:h"), 15840);
    assert!(xml.contains("Owned wide page"));
    assert!(
        xml.contains("w:sz w:val=\"32\""),
        "20pt font must scale by0.792: {xml}"
    );
}

/// 既にWord範囲内の原寸は拡大せず、本文とfontを保つ。
#[test]
fn supported_page_is_not_scaled() {
    let source = pdf(1000, 500);
    let xml = document(convert_minimum_pdf_bytes_to_docx_bytes("normal.pdf", &source).unwrap());
    assert_eq!(attribute(&xml, "w:pgSz", "w:w"), 20000);
    assert_eq!(attribute(&xml, "w:pgSz", "w:h"), 10000);
    assert!(xml.contains("w:sz w:val=\"40\""));
}

/// 原寸変更をstrictで受け入れず、既存出力とPDFを保持する。
#[test]
fn strict_rejects_page_fit_before_publication() {
    let source = pdf(2000, 1000);
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.pdf");
    let output = directory.path().join("result.docx");
    fs::write(&input, &source).unwrap();
    fs::write(&output, b"keep").unwrap();
    let options = ParseOptions {
        strict: true,
        ..Default::default()
    };
    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input.clone(),
        output_docx_path: output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    };
    let result = convert_minimum_pdf_to_docx_with_options(&request, &options);
    assert!(result.is_err(), "strict published altered physical size");
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("page_resized_for_word")
    );
    assert_eq!(fs::read(output).unwrap(), b"keep");
    assert_eq!(fs::read(input).unwrap(), source);
    assert!(
        convert_minimum_pdf_bytes_to_docx_bytes_with_options("strict.pdf", &source, &options)
            .is_err()
    );
}

/// 生成した1px PNGを表示寸法だけ拡大する。保存fixtureは追加しない。
fn one_by_one_png_bytes() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xff, 0xff, 0x3f, 0x00, 0x05, 0xfe, 0x02, 0xfe, 0xdc, 0xcc, 0x59, 0xe7, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]
}

/// 画像ページのpage枠と両方のdrawing extentを等比縮小し、PNG byte列は保持する。
#[test]
fn rendered_page_fits_image_and_page_with_original_payload() {
    let png = one_by_one_png_bytes();
    let output = convert_rendered_png_pages_to_docx_bytes(&[RenderedPageImage::new(
        png.clone(),
        3000,
        1500,
    )])
    .unwrap();
    let xml = document(output.clone());
    assert_eq!(attribute(&xml, "w:pgSz", "w:w"), 31680);
    assert_eq!(attribute(&xml, "w:pgSz", "w:h"), 15840);
    assert_eq!(attribute(&xml, "wp:extent", "cx"), 31680 * 635);
    assert_eq!(attribute(&xml, "wp:extent", "cy"), 15840 * 635);
    assert_eq!(attribute(&xml, "a:ext", "cx"), 31680 * 635);
    let mut archive = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let name = archive
        .file_names()
        .find(|n| n.starts_with("word/media/") && !n.ends_with('/'))
        .unwrap()
        .to_owned();
    let mut embedded = Vec::new();
    archive
        .by_name(&name)
        .unwrap()
        .read_to_end(&mut embedded)
        .unwrap();
    assert_eq!(embedded, png);
}

/// 通常変換のreportに原寸変更を明記し、同じ診断の追加を重複させない。
#[test]
fn default_report_exposes_physical_resize_without_mutating_parser_geometry() {
    let original = pdf(2000, 1000);
    let directory = tempfile::tempdir().unwrap();
    let request = MinimumPdfToDocxRequest {
        input_pdf_path: directory.path().join("source.pdf"),
        output_docx_path: directory.path().join("result.docx"),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    };
    fs::write(&request.input_pdf_path, &original).unwrap();
    let report =
        convert_minimum_pdf_to_docx_with_options(&request, &ParseOptions::default()).unwrap();
    assert_eq!(
        report
            .quality
            .issues
            .iter()
            .filter(|i| matches!(
                i,
                jade_pdf2docx_parser::QualityIssue::PageResizedForWord { page: 0 }
            ))
            .count(),
        1
    );
    let parsed = jade_pdf2docx_parser::parse_pdf_bytes("original.pdf", &original).unwrap();
    assert_eq!(parsed.pages[0].width, 2000.0);
    assert!(parsed.quality.issues.is_empty());
    assert_eq!(
        jade_pdf2docx_docx::docx_quality_report(&parsed),
        report.quality
    );
    assert_eq!(fs::read(request.input_pdf_path).unwrap(), original);
}

/// 大判の画像1枚だけを持つ5object PDFを生成する。
fn scan_pdf() -> Vec<u8> {
    use std::io::Write;
    let content = "q 2000 0 0 1000 0 0 cm /Im1 Do Q";
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"abc").unwrap();
    let compressed = encoder.finish().unwrap();
    let mut image=format!("<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",compressed.len()).into_bytes();
    image.extend(compressed);
    image.extend_from_slice(b"\nendstream");
    owned_byte_objects(&[b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 2000 1000] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()).into_bytes(),image])
}

/// OCRの原寸pixel座標を出力だけで縮小し、読み順を変えずstrictを推論前に拒否する。
#[test]
fn whole_page_ocr_scales_frames_only_after_recognition_and_strict_skips_callback() {
    use jade_pdf2docx_docx::{
        OcrPageContent, PageOcrLimits, PositionedOcrLine,
        convert_minimum_pdf_to_docx_with_bounded_page_ocr_content,
    };
    let source = scan_pdf();
    let directory = tempfile::tempdir().unwrap();
    let request = MinimumPdfToDocxRequest {
        input_pdf_path: directory.path().join("scan.pdf"),
        output_docx_path: directory.path().join("result.docx"),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    };
    fs::write(&request.input_pdf_path, &source).unwrap();
    fs::write(&request.output_docx_path, b"keep").unwrap();
    let limits = PageOcrLimits {
        dpi: 72,
        ..Default::default()
    };
    let result = convert_minimum_pdf_to_docx_with_bounded_page_ocr_content(
        &request,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
        &limits,
        |_| panic!("strict must reject resizing before OCR"),
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("page_resized_for_word")
    );
    assert_eq!(fs::read(&request.output_docx_path).unwrap(), b"keep");
    let report = convert_minimum_pdf_to_docx_with_bounded_page_ocr_content(
        &request,
        &ParseOptions::default(),
        &limits,
        |raster| {
            assert_eq!(raster.page, 0);
            Ok(OcrPageContent::PositionedText(vec![
                PositionedOcrLine {
                    text: "Right first".into(),
                    bounds: [1500, 100, 1800, 120],
                },
                PositionedOcrLine {
                    text: "Left second".into(),
                    bounds: [100, 100, 400, 120],
                },
            ]))
        },
    )
    .unwrap();
    assert!(
        report.quality.issues.iter().all(|i| matches!(
            i,
            jade_pdf2docx_parser::QualityIssue::PageResizedForWord { page: 0 }
        )),
        "{:?}",
        report.quality
    );
    let out = document(fs::read(&request.output_docx_path).unwrap());
    assert_eq!(attribute(&out, "w:framePr", "w:x"), 23760);
    assert_eq!(attribute(&out, "w:framePr", "w:y"), 1584);
    assert_eq!(attribute(&out, "w:framePr", "w:w"), 4752);
    assert_eq!(attribute(&out, "w:framePr", "w:h"), 317);
    assert!(out.find("Right first").unwrap() < out.find("Left second").unwrap());
    assert!(out.contains(r#"<w:sz w:val="17"/>"#));
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), source);
}
