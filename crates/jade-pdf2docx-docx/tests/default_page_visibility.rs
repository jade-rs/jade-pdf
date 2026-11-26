//! 原本の表示枠の外にある要素を、通常変換でページ内へ再配置しない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/source_image_pdf.rs"]
mod fixture;

/// CropBoxを省略した正式MediaBoxと、表示内本文・任意の表示外本文や画像を持つ所有原本。
fn source(rotation: i32, outside_text: bool, image_x: Option<i32>) -> Vec<u8> {
    source_with_visible(rotation, outside_text, image_x, true)
}

/// 可視本文を持たない対照も同じ正式xrefで作る。
fn source_with_visible(
    rotation: i32,
    outside_text: bool,
    image_x: Option<i32>,
    visible: bool,
) -> Vec<u8> {
    source_with_geometry(rotation, outside_text, image_x.map(|x| (x, 100)), visible)
}

/// 上下左右に配置した画像の交差を、同じ非正方形MediaBoxで検査する。
fn source_with_geometry(
    rotation: i32,
    outside_text: bool,
    image: Option<(i32, i32)>,
    visible: bool,
) -> Vec<u8> {
    let mut content = if visible {
        "BT /F1 12 Tf 20 30 Td (Visible) Tj ET ".to_string()
    } else {
        String::new()
    };
    if outside_text {
        content.push_str("BT /F1 12 Tf 400 30 Td (Outside) Tj ET ");
    }
    if let Some((x, y)) = image {
        content.push_str(&format!("q 20 0 0 10 {x} {y} cm /Im Do Q "));
    }
    fixture::rgb_with_page_geometry(200, 300, rotation, content.as_bytes())
}

/// 公開byte変換が実際に作るWord本文を確認する。
fn document(input: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-default-page-visibility.pdf", input)
        .unwrap();
    xml(bytes)
}

/// fileとbytesが生成したWord本文を、同じZIP読取で検査する。
fn xml(bytes: Vec<u8>) -> String {
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

/// 全要素が表示外でも、テキストプレビューの代替経路で本文へ復活させない。
#[test]
fn entirely_off_page_document_does_not_fall_back_to_hidden_preview() {
    for rotation in [0, 90, 180, 270] {
        let xml = document(&source_with_visible(rotation, true, Some(300), false));
        assert!(!xml.contains("Outside"), "rotation={rotation}");
        assert!(!xml.contains("<w:drawing>"), "rotation={rotation}");
    }
}

/// native file経路でも表示外を再配置せず、入力byteと表示内の対照を維持する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_file_default_keeps_only_the_displayed_source_elements() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    for rotation in [0, 90, 180, 270] {
        for outside in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let input = source(rotation, outside, Some(if outside { 300 } else { 50 }));
            let input_path = dir.path().join("owned-original.pdf");
            let output_path = dir.path().join("converted.docx");
            std::fs::write(&input_path, &input).unwrap();
            convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
                input_pdf_path: input_path.clone(),
                output_docx_path: output_path.clone(),
                ocr_text_lines: None,
                ocr_table: None,
                clean_output: true,
            })
            .unwrap();
            assert_eq!(std::fs::read(input_path).unwrap(), input);
            let xml = xml(std::fs::read(output_path).unwrap());
            assert!(!xml.contains("Outside"));
            assert_eq!(xml.matches("<w:drawing>").count(), usize::from(!outside));
            assert_eq!(xml.matches("Visible").count(), 1);
        }
    }
}

/// 回転原本の縦長boundsを通常の横書きへ戻す対照で、残る単語の幅を確保する。
#[test]
fn remaining_right_anchored_word_has_room_after_hidden_elements_are_removed() {
    let original = parse_pdf_bytes("owned-horizontal-word.pdf", &source(0, false, None)).unwrap();
    let word = original
        .text_elements
        .iter()
        .find(|t| t.text == "Visible")
        .unwrap();
    let expected = ((word.bounds.x1 - word.bounds.x0) * 20.).ceil() as i32;
    let xml = document(&source(270, true, Some(300)));
    let mut reader = quick_xml::Reader::from_str(&xml);
    let (mut width, mut left, mut right, mut indent_left, mut indent_right) = (0, 0, 0, 0, 0);
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Empty(tag) => {
                let value = |key: &[u8]| {
                    tag.attributes()
                        .map(Result::unwrap)
                        .find(|a| a.key.as_ref() == key)
                        .map(|a| {
                            std::str::from_utf8(&a.value)
                                .unwrap()
                                .parse::<i32>()
                                .unwrap()
                        })
                        .unwrap_or(0)
                };
                match tag.name().as_ref() {
                    b"w:pgSz" => width = value(b"w:w"),
                    b"w:pgMar" => {
                        left = value(b"w:left");
                        right = value(b"w:right");
                    }
                    b"w:ind" => {
                        indent_left = value(b"w:left");
                        indent_right = value(b"w:right");
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    assert!(
        width - left - right - indent_left - indent_right >= expected,
        "Word flow width must fit the same source word without hidden content padding"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 直角回転と上下左右の配置によらず、表示枠へ一部でも交差する画像だけを残す。
    #[test]
    fn default_image_retention_follows_source_intersection(turn in 0i32..4,x in -60i32..320,y in -60i32..380) {
        let expected=x<200&&x+20>0&&y<300&&y+10>0;
        let xml=document(&source_with_geometry(turn*90,true,Some((x,y)),true));
        prop_assert_eq!(xml.matches("<w:drawing>").count(),usize::from(expected));
        prop_assert_eq!(xml.matches("Visible").count(),1);
        prop_assert!(!xml.contains("Outside"));
    }
}
