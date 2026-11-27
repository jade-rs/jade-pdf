//! 白い紙を表す画像を、本文のページを押し出す独立した写真に変換しない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};

#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/source_image_pdf.rs"]
mod fixture;

/// 前面・背景を同じ原本の描画命令で比較できる固定本文。
const BODY: &str = "BT /F1 12 Tf 20 62 Td (Visible body) Tj ET";
/// 100pt角のPDF用紙全体へ、Image XObjectを描画する。
const PAGE_IMAGE: &str = "q 100 0 0 100 0 0 cm /Im Do Q";

/// 同じ原本の本文・画像を任意の順番と色で描く。
fn source(draw: &str, samples: &[u8; 6]) -> Vec<u8> {
    fixture::source_image_pdf_with_content(
        "/ColorSpace /DeviceRGB /Filter /FlateDecode",
        &fixture::compressed(samples),
        "",
        8,
        draw.as_bytes(),
    )
}

/// 生成されたパッケージの本文XMLを読む。
fn xml(docx: Vec<u8>) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(docx)).unwrap();
    let mut text = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// メモリから通常のWord本文を生成する。
fn convert(bytes: &[u8]) -> String {
    xml(convert_minimum_pdf_bytes_to_docx_bytes("owned-white-background.pdf", bytes).unwrap())
}

/// 先に描いた白画像だけを省き、本文の書式と配置を同じ原本の対照と一致させる。
#[test]
fn opaque_white_page_background_preserves_the_body_layout() {
    let original = source(&format!("{PAGE_IMAGE} {BODY}"), &[255; 6]);
    let text_only = source(BODY, &[255; 6]);
    let parsed =
        jade_pdf2docx_parser::parse_pdf_bytes("owned-white-background.pdf", &original).unwrap();
    assert_eq!(
        parsed.image_elements.len(),
        1,
        "the parser retains the source raster"
    );
    let actual = convert(&original);
    assert!(
        !actual.contains("<w:drawing>"),
        "the blank paper raster must not reserve a second page"
    );
    assert_eq!(actual, convert(&text_only));
}

/// 前面の白、文字を含まないページ、部分画像、白以外、切り抜き付きの画像は保持する。
#[test]
fn other_page_images_are_retained() {
    for (content, samples) in [
        (format!("{BODY} {PAGE_IMAGE}"), [255; 6]),
        (PAGE_IMAGE.to_owned(), [255; 6]),
        (format!("q 35 0 0 25 5 5 cm /Im Do Q {BODY}"), [255; 6]),
        (format!("{PAGE_IMAGE} {BODY}"), [0, 255, 255, 255, 255, 255]),
        (
            format!("q 0 0 40 40 re W n 100 0 0 100 0 0 cm /Im Do Q {BODY}"),
            [255; 6],
        ),
    ] {
        assert!(
            convert(&source(&content, &samples)).contains("<w:drawing>"),
            "{content}"
        );
    }
}

/// 透明度付きの白は、同じ矩形にあっても不透明な紙と見なさない。
#[test]
fn a_white_image_with_nonopaque_samples_is_retained() {
    let original = fixture::source_image_pdf_with_content(
        "/ColorSpace /DeviceRGB /Filter /FlateDecode /SMask 5 0 R",
        &fixture::compressed(&[255; 6]),
        "",
        8,
        format!("{PAGE_IMAGE} {BODY}").as_bytes(),
    );
    assert!(convert(&original).contains("<w:drawing>"));
}

/// Contents配列でも描画状態と順序を保ち、先行streamの文字を覆う白画像を維持する。
#[test]
fn document_contents_preserve_which_paint_is_first() {
    let background = fixture::rgb_image_with_two_contents(
        &[255; 6],
        b"q 100 0 0 100 0 0 cm ",
        format!("/Im Do Q {BODY}").as_bytes(),
    );
    assert_eq!(convert(&background), convert(&source(BODY, &[255; 6])));
    let foreground =
        fixture::rgb_image_with_two_contents(&[255; 6], BODY.as_bytes(), PAGE_IMAGE.as_bytes());
    assert!(convert(&foreground).contains("<w:drawing>"));
}

/// nativeのファイル変換も先に描いた白い紙をレイアウトから外す。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn file_conversion_omits_the_verified_white_background() {
    use jade_pdf2docx_docx::{
        MinimumPdfToDocxRequest, PageOcrLimits, ParseOptions, convert_minimum_pdf_to_docx,
        convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("owned-white-first.pdf");
    let output = dir.path().join("converted.docx");
    std::fs::write(&input, source(&format!("{PAGE_IMAGE} {BODY}"), &[255; 6])).unwrap();
    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input,
        output_docx_path: output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    };
    convert_minimum_pdf_to_docx(&request).unwrap();
    let normal = std::fs::read(&output).unwrap();
    let actual = xml(normal.clone());
    assert!(!actual.contains("<w:drawing>"));
    assert_eq!(actual.matches("Visible body").count(), 1);
    convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
        &request,
        &ParseOptions {
            strict: true,
            ..ParseOptions::default()
        },
        &PageOcrLimits::default(),
        |_| Err("the white sheet does not contain text to recognize".into()),
    )
    .unwrap();
    assert_eq!(std::fs::read(output).unwrap(), normal);
}
