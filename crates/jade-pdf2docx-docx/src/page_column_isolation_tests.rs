//! 他ページの画像が、画像のないページの実段組を無効化しない回帰。
use super::*;
use proptest::prelude::*;
use std::io::{Cursor, Read};

#[path = "floating_image_anchor_tests.rs"]
mod floating_images;

#[path = "source_image_row_tests.rs"]
mod source_image_rows;

#[path = "page_chrome_occurrence_tests.rs"]
mod page_chrome_occurrences;

#[path = "page_table_column_tests.rs"]
mod tables;

#[path = "narrow_text_gutter_tests.rs"]
mod narrow_gutters;

#[path = "column_flow_tests.rs"]
mod column_flow;

#[path = "inline_baseline_tests.rs"]
mod inline_baselines;
#[path = "inline_script_gutter_tests.rs"]
mod script_gutters;
#[path = "inline_script_row_tests.rs"]
mod script_rows;

#[path = "separated_box_table_tests.rs"]
mod separated_boxes;

/// 外部資料を使わず、二つの段と別ページの所有画像を持つ解析結果を作る。
fn source(page_count: usize, image_pages: &[usize]) -> ParsedDocument {
    assert!((2..=5).contains(&page_count));
    let mut parsed = ParsedDocument {
        quality: Default::default(),
        path: "owned-page-columns.pdf".into(),
        byte_len: 1,
        header: "%PDF-1.7".into(),
        version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
        page_count,
        pages: (0..page_count)
            .map(|page_number| ParsedPage {
                page_number,
                width: 612.,
                height: 792.,
                rotation: 0,
            })
            .collect(),
        graphics: Vec::new(),
        text_elements: Vec::new(),
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: String::new(),
    };
    for page in 0..page_count {
        for (column, x) in [("LEFT", 48.), ("RIGHT", 330.)] {
            for row in 0..2 {
                let y = 695. - row as f32 * 25.;
                parsed.text_elements.push(ParsedTextElement {
                    font_weight: None,
                    baseline_y: Some(y),
                    page,
                    bounds: ParsedBounds {
                        x0: x,
                        y0: y,
                        x1: x + 65.,
                        y1: y + 11.,
                    },
                    font_size: 11.,
                    font_name: Some("Helvetica".into()),
                    fill_color: None,
                    text: format!("PAGE{} {column} {}", char::from(b'A' + page as u8), row + 1),
                });
            }
        }
    }
    for &page in image_pages {
        // 自作の1px白色PNG。外部fontや画像の再利用はない。
        let png_bytes = vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 255,
            255, 63, 0, 5, 254, 2, 254, 13, 239, 70, 184, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
            130,
        ];
        parsed.image_elements.push(ParsedImageElement {
            page,
            bounds: ParsedBounds {
                x0: 48.,
                y0: 180.,
                x1: 96.,
                y1: 228.,
            },
            png_bytes,
            width_px: 64,
            height_px: 64,
            floating: false,
            rotation_degrees: 0,
        });
    }
    parsed
}

/// 所有画像経路の実際のDOCXを開き、本文XMLと画像数を読む。
fn package(image_page: Option<usize>) -> (String, usize) {
    package_source(source(2, &image_page.into_iter().collect::<Vec<_>>()))
}

/// 画像の所有権を移した解析結果をZIP化し、画像と本文を検証可能にする。
fn package_source(mut parsed: ParsedDocument) -> (String, usize) {
    let docx = build_structured_docx_from_owned_images(&mut parsed, &None, &[]).unwrap();
    assert!(
        parsed
            .image_elements
            .iter()
            .all(|image| image.png_bytes.is_empty())
    );
    let bytes = pack_docx_to_bytes(docx, &[]).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let images = zip
        .file_names()
        .filter(|name| name.starts_with("word/media/") && name.ends_with(".png"))
        .count();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    (xml, images)
}

proptest! {
    /// ページ数と画像ページの組合せが変わっても、画像のない各ページの段組を保つ。
    #[test]
    fn image_page_permutations_preserve_unaffected_columns(
        page_count in 2usize..=5,
        image_mask in 0u8..32,
    ) {
        let image_pages = (0..page_count)
            .filter(|page| image_mask & (1 << page) != 0)
            .collect::<Vec<_>>();
        let (xml, images) = package_source(source(page_count, &image_pages));
        prop_assert_eq!(xml.matches(r#"w:num="2""#).count(), page_count - image_pages.len());
        prop_assert_eq!(xml.matches("<w:drawing>").count(), image_pages.len());
        prop_assert_eq!(images > 0, !image_pages.is_empty());
    }
}

/// 前後どちらのページに画像があっても、もう一方の実段組と画像所属を保つ。
#[test]
fn another_pages_image_preserves_columns_and_stays_before_its_section_boundary() {
    let (control, _) = package(None);
    assert_eq!(control.matches(r#"w:num="2""#).count(), 2);
    for image_page in 0..2 {
        let (xml, images) = package(Some(image_page));
        assert_eq!(images, 1, "owned image payload must survive packaging");
        assert_eq!(
            xml.matches(r#"w:num="2""#).count(),
            1,
            "image on page {image_page} must not erase the other page's columns: {xml}"
        );
        let drawing = xml.find("<w:drawing>").unwrap();
        let boundary = xml.find("<w:sectPr>").unwrap();
        // sectPrは段落の先頭側のpPrに保存される。境界段落そのものの画像も前ページに属する。
        let boundary_paragraph_end = boundary + xml[boundary..].find("</w:p>").unwrap();
        assert_eq!(
            drawing < boundary_paragraph_end,
            image_page == 0,
            "image must stay in its original page section: {xml}"
        );
    }
}

/// 対象の所有テキスト段落から、符号つき左indentを独立して読む。
fn indent_for_text(xml: &str, target: &str) -> i32 {
    use quick_xml::{Reader, events::Event};
    let mut reader = Reader::from_str(xml);
    let mut text = String::new();
    let mut indent = 0;
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) if e.name().as_ref() == b"w:p" => {
                text.clear();
                indent = 0;
            }
            Event::Start(e) if e.name().as_ref() == b"w:t" => in_text = true,
            Event::Empty(e) if e.name().as_ref() == b"w:ind" => {
                for attr in e.attributes() {
                    let attr = attr.unwrap();
                    if attr.key.as_ref() == b"w:left" {
                        indent = std::str::from_utf8(&attr.value).unwrap().parse().unwrap();
                    }
                }
            }
            Event::Text(e) if in_text => text.push_str(&e.decode().unwrap()),
            Event::End(e) if e.name().as_ref() == b"w:t" => in_text = false,
            Event::End(e) if e.name().as_ref() == b"w:p" && text == target => return indent,
            Event::Eof => panic!("missing owned paragraph {target}"),
            _ => {}
        }
    }
}

/// 均等段の開始点より左・右の原本行を、符号を保ったindentで配置する。
#[test]
fn unequal_source_gutter_keeps_signed_right_column_displacement() {
    // Word領域:幅12240twip、左960、右0、段間425。右段原点は6812.5twip。
    for (source_left, expected_indent) in [(330., -213), (360., 388)] {
        let mut parsed = source(2, &[1]);
        for text in &mut parsed.text_elements {
            if text.text.contains("RIGHT") {
                let shift = source_left - text.bounds.x0;
                text.bounds.x0 += shift;
                text.bounds.x1 += shift;
            }
        }
        let (xml, _) = package_source(parsed);
        assert_eq!(
            indent_for_text(&xml, "PAGEA RIGHT 1"),
            expected_indent,
            "source column x={source_left} must survive Word column placement"
        );
    }
}

/// 最初の実sectionに保存された整数twip属性を、生成XMLから読む。
fn first_section_attribute(xml: &str, element: &[u8], attribute: &[u8]) -> f64 {
    use quick_xml::{Reader, events::Event};
    let mut reader = Reader::from_str(xml);
    let mut in_section = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) if e.name().as_ref() == b"w:sectPr" => in_section = true,
            Event::Empty(e) if in_section && e.name().as_ref() == element => {
                for attr in e.attributes() {
                    let attr = attr.unwrap();
                    if attr.key.as_ref() == attribute {
                        return std::str::from_utf8(&attr.value).unwrap().parse().unwrap();
                    }
                }
            }
            Event::Eof => panic!("missing owned section attribute"),
            _ => {}
        }
    }
}

proptest! {
    /// ページ幅・余白が端数でも、実XMLのWord領域とindentの和が原本位置を保つ。
    #[test]
    fn fractional_section_geometry_preserves_source_column_x(
        // 正のf32の全bit表現を同じ半開区間から生成し、float sampler内部の丸めpanicを避ける。
        width in (600.0_f32.to_bits()..1400.0_f32.to_bits()).prop_map(f32::from_bits),
        left_fraction in (0.02_f32.to_bits()..0.12_f32.to_bits()).prop_map(f32::from_bits),
        right_fraction in (0.58_f32.to_bits()..0.82_f32.to_bits()).prop_map(f32::from_bits),
    ) {
        let left = width * left_fraction;
        let right = width * right_fraction;
        let mut parsed = source(2, &[1]);
        for page in &mut parsed.pages { page.width = width; }
        for text in &mut parsed.text_elements {
            let x = if text.text.contains("RIGHT") { right } else { left };
            text.bounds.x0 = x;
            text.bounds.x1 = x + 65.;
        }
        let (xml, _) = package_source(parsed);
        let word_width = first_section_attribute(&xml, b"w:pgSz", b"w:w");
        let word_left = first_section_attribute(&xml, b"w:pgMar", b"w:left");
        let word_gap = first_section_attribute(&xml, b"w:cols", b"w:space");
        let word_right_origin = (word_width + word_left + word_gap) / 2.;
        let position = word_right_origin + f64::from(indent_for_text(&xml, "PAGEA RIGHT 1"));
        let error = (position - f64::from(right) * 20.).abs();
        prop_assert!(error <= 0.5001, "Word region quantisation must preserve source x; error={error}twip");
    }
}
