//! 他ページの画像が、画像のないページの実段組を無効化しない回帰。
use super::*;
use proptest::prelude::*;
use std::io::{Cursor, Read};

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
