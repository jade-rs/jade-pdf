//! 原本の表示枠の外にある要素を、通常変換でページ内へ再配置しない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use jade_pdf2docx_parser::parse_pdf_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/source_image_pdf.rs"]
mod fixture;

/// CropBoxを省略した正式MediaBoxと、表示内本文・任意の表示外本文や画像を持つ所有原本。
fn source(rotation: i32, outside_text: bool, image_x: Option<i32>) -> Vec<u8> {
    let mut content = "BT /F1 12 Tf 20 30 Td (Visible) Tj ET ".to_string();
    if outside_text {
        content.push_str("BT /F1 12 Tf 400 30 Td (Outside) Tj ET ");
    }
    if let Some(x) = image_x {
        content.push_str(&format!("q 20 0 0 10 {x} 100 cm /Im Do Q "));
    }
    fixture::rgb_with_page_geometry(200, 300, rotation, content.as_bytes())
}

/// 公開byte変換が実際に作るWord本文を確認する。
fn document(input: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-default-page-visibility.pdf", input)
        .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 元のMediaBoxと交差しない文字を、回転後も可視のWord本文へ入れない。
#[test]
fn entirely_off_page_text_is_not_reflowed_into_word() {
    for rotation in [0, 90, 180, 270] {
        let xml = document(&source(rotation, true, None));
        assert!(!xml.contains("Outside"), "rotation={rotation}");
        assert_eq!(xml.matches("Visible").count(), 1);
    }
}

/// parserが保持する表示外画像を、通常のWord本文へ再配置しない。
#[test]
fn entirely_off_page_image_is_not_reflowed_into_word() {
    for rotation in [0, 90, 180, 270] {
        let input = source(rotation, false, Some(300));
        let parsed = parse_pdf_bytes("owned-outside-image.pdf", &input).unwrap();
        assert_eq!(
            parsed.image_elements.len(),
            1,
            "parser IR remains available"
        );
        let xml = document(&input);
        assert!(!xml.contains("<w:drawing>"), "rotation={rotation}");
        assert_eq!(xml.matches("Visible").count(), 1);
    }
}

/// 同じページの表示内の画像と文字を、回転や通常のOCR確認要求で消さない。
#[test]
fn visible_image_and_text_remain_in_default_word() {
    for rotation in [0, 90, 180, 270] {
        let xml = document(&source(rotation, false, Some(50)));
        assert_eq!(xml.matches("<w:drawing>").count(), 1, "rotation={rotation}");
        assert_eq!(xml.matches("Visible").count(), 1);
    }
}
