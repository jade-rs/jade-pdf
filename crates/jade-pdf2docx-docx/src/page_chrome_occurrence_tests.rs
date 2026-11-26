//! 同じ文字列の本文題名と反復見出しを、書式と所属ページ付きで保持する回帰。
use super::*;

/// 初ページの18pt題名と後続ページの9pt見出しを同じ文字列で作る。
fn title_and_running_headers() -> ParsedDocument {
    let mut parsed = source(3, &[]);
    parsed.text_elements.retain(|text| text.bounds.x0 == 48.);
    for text in &mut parsed.text_elements {
        let y = if text.text.ends_with('1') { 600. } else { 575. };
        text.baseline_y = Some(y);
        text.bounds.y0 = y;
        text.bounds.y1 = y + 11.;
    }
    for page in 0..3 {
        let (size, top) = if page == 0 { (18., 712.) } else { (9., 752.) };
        parsed.text_elements.push(ParsedTextElement {
            source_background: None,
            fill_stroke_width: None,
            font_weight: None,
            page,
            baseline_y: Some(top - size),
            bounds: ParsedBounds {
                x0: 48.,
                y0: top - size,
                x1: 198.,
                y1: top,
            },
            font_size: size,
            font_name: Some("Helvetica".into()),
            fill_color: None,
            text: "SHARED TITLE".into(),
        });
    }
    parsed
}

/// 実際のpackageを開き、本文と見出しpartを検証可能な文字列にする。
fn parts(mut parsed: ParsedDocument) -> BTreeMap<String, String> {
    let docx = build_structured_docx_from_owned_images(&mut parsed, &None, &[]).unwrap();
    let bytes = pack_docx_to_bytes(docx, &[]).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let names: Vec<_> = zip
        .file_names()
        .filter(|name| {
            *name == "word/document.xml"
                || *name == "word/_rels/document.xml.rels"
                || name.starts_with("word/header")
                || name.starts_with("word/footer")
        })
        .map(str::to_owned)
        .collect();
    names
        .into_iter()
        .map(|name| {
            let mut xml = String::new();
            zip.by_name(&name)
                .unwrap()
                .read_to_string(&mut xml)
                .unwrap();
            (name, xml)
        })
        .collect()
}

/// 属性順序に依存せず、対象タグの属性を読む。
fn attribute(xml: &str, element: &[u8], key: &[u8]) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                if e.local_name().as_ref() == element =>
            {
                return e
                    .attributes()
                    .map(Result::unwrap)
                    .find(|a| a.key.as_ref() == key)
                    .map(|a| String::from_utf8(a.value.to_vec()).unwrap());
            }
            quick_xml::events::Event::Eof => return None,
            _ => (),
        }
    }
}

/// sectionのheader参照を実際のZIP部品へ解決する。
fn header_part<'a>(parts: &'a BTreeMap<String, String>, section: &str) -> Option<&'a str> {
    chrome_part(parts, section, b"headerReference")
}

/// sectionのheader/footer参照を、実際のrelationship targetへ解決する。
fn chrome_part<'a>(
    parts: &'a BTreeMap<String, String>,
    section: &str,
    reference: &[u8],
) -> Option<&'a str> {
    let id = attribute(section, reference, b"r:id")?;
    let relationships = &parts["word/_rels/document.xml.rels"];
    let mut reader = quick_xml::Reader::from_str(relationships);
    let target = loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                let attributes: BTreeMap<_, _> = e
                    .attributes()
                    .map(Result::unwrap)
                    .map(|a| {
                        (
                            a.key.as_ref().to_vec(),
                            String::from_utf8(a.value.to_vec()).unwrap(),
                        )
                    })
                    .collect();
                if attributes.get(b"Id".as_slice()) == Some(&id) {
                    break attributes[b"Target".as_slice()].clone();
                }
            }
            quick_xml::events::Event::Eof => panic!("missing header relationship"),
            _ => (),
        }
    };
    Some(
        parts
            .get(&format!("word/{target}"))
            .expect("header relationship must resolve"),
    )
}

/// 前ページに見出しがあっても、本文題名だけのページに引き継がせない。
#[test]
fn singleton_title_between_running_headers_explicitly_clears_inheritance() {
    let mut parsed = title_and_running_headers();
    for text in &mut parsed.text_elements {
        if text.text == "SHARED TITLE" {
            let size = if text.page == 1 { 18. } else { 9. };
            text.font_size = size;
            text.bounds.y1 = 752.;
            text.bounds.y0 = 752. - size;
            text.baseline_y = Some(752. - size);
        }
    }
    let parts = parts(parsed);
    let sections: Vec<_> = parts["word/document.xml"]
        .split("<w:sectPr")
        .skip(1)
        .map(|part| part.split("</w:sectPr>").next().unwrap())
        .collect();
    let middle = header_part(&parts, sections[1]);
    assert!(
        middle.is_some(),
        "middle title page must explicitly clear header inheritance"
    );
    assert!(!middle.unwrap().contains("SHARED TITLE"));
}

/// 初ページの本文題名を共有headerへ移さず、後続ページに9ptの見出しを持たせる。
#[test]
fn first_title_and_later_running_headers_keep_their_page_and_style() {
    let parts = parts(title_and_running_headers());
    let body = &parts["word/document.xml"];
    assert_eq!(
        body.matches("SHARED TITLE").count(),
        1,
        "first-page title must remain editable body text"
    );
    assert!(body.contains("w:sz w:val=\"36\""));
    let headers: Vec<_> = parts
        .iter()
        .filter(|(name, _)| name.starts_with("word/header"))
        .collect();
    assert!(
        !headers.is_empty(),
        "repeated9pt occurrences must still be real headers"
    );
    for (_, xml) in headers {
        assert!(xml.contains("SHARED TITLE") && xml.contains("w:sz w:val=\"18\""));
        assert!(!xml.contains("w:sz w:val=\"36\""));
    }
    let sections: Vec<_> = body
        .split("<w:sectPr")
        .skip(1)
        .map(|part| part.split("</w:sectPr>").next().unwrap())
        .collect();
    assert_eq!(sections.len(), 3);
    assert!(!sections[0].contains("w:headerReference"));
    assert!(
        sections[1..]
            .iter()
            .all(|section| section.contains("w:headerReference"))
    );
}

/// 本文が二段組でも初ページの題名を後続ページのheaderから切り離す。
#[test]
fn column_sections_do_not_inherit_later_pages_running_header_on_first_page() {
    let mut parsed = title_and_running_headers();
    let right: Vec<_> = parsed
        .text_elements
        .iter()
        .filter(|line| line.text.starts_with("PAGE"))
        .cloned()
        .map(|mut line| {
            line.text = line.text.replace("LEFT", "RIGHT");
            line.bounds.x0 += 282.;
            line.bounds.x1 += 282.;
            line
        })
        .collect();
    parsed.text_elements.extend(right);
    let parts = parts(parsed);
    let body = &parts["word/document.xml"];
    assert!(body.contains("w:num=\"2\""), "source columns must survive");
    let first = body
        .split("<w:sectPr")
        .nth(1)
        .unwrap()
        .split("</w:sectPr>")
        .next()
        .unwrap();
    assert!(
        !first.contains("w:headerReference"),
        "first-page column sections must not inherit a later running header"
    );
}

/// ページ数に依存せず、交互の書式を持つ実際のsourceを自作する。
fn alternating_headers(page_count: usize, columns: bool) -> ParsedDocument {
    let template = source(2, &[]);
    let mut parsed = template.clone();
    parsed.page_count = page_count;
    parsed.pages.clear();
    parsed.text_elements.clear();
    for page in 0..page_count {
        let mut geometry = template.pages[0].clone();
        geometry.page_number = page;
        parsed.pages.push(geometry);
        for text in template.text_elements.iter().filter(|text| text.page == 0) {
            if !columns && text.bounds.x0 > 48. {
                continue;
            }
            let mut text = text.clone();
            text.page = page;
            text.text = text.text.replace("PAGEA", &format!("PAGE{page}"));
            parsed.text_elements.push(text);
        }
        let mut header = template.text_elements[0].clone();
        header.page = page;
        header.text = "SHARED TITLE".into();
        header.font_size = if page % 2 == 0 { 9. } else { 10. };
        header.bounds.y1 = 752.;
        header.bounds.y0 = 752. - header.font_size;
        header.baseline_y = Some(header.bounds.y0);
        parsed.text_elements.push(header);
    }
    parsed
}

/// 10番目以降のheader部品も文字列順ではなくrelationshipの番号に対応する。
#[test]
fn twelve_page_headers_resolve_to_the_correct_styles() {
    let parts = parts(alternating_headers(12, false));
    let sections: Vec<_> = parts["word/document.xml"]
        .split("<w:sectPr")
        .skip(1)
        .map(|part| part.split("</w:sectPr>").next().unwrap())
        .collect();
    assert_eq!(sections.len(), 12);
    for (page, section) in sections.iter().enumerate() {
        let header = header_part(&parts, section).unwrap();
        let size = if page % 2 == 0 { 18 } else { 20 };
        assert!(
            header.contains(&format!("w:sz w:val=\"{size}\"")),
            "header part style must match source page {page}"
        );
    }
}

proptest! {
    /// 原本ページ数と段組数を変えても、本文所属と両端の見出し書式を保持する。
    #[test]
    fn prop_chrome_parts_match_source_pages(count in 4usize..25, columns in any::<bool>()) {
        let mut parsed = alternating_headers(count, columns);
        let footers: Vec<_> = parsed.text_elements.iter()
            .filter(|text| text.text == "SHARED TITLE").cloned()
            .map(|mut text| {
                text.text = "SHARED FOOTER".into();
                text.bounds.y1 = 40.;
                text.bounds.y0 = 40. - text.font_size;
                text.baseline_y = Some(text.bounds.y0);
                text
            }).collect();
        parsed.text_elements.extend(footers);
        let parts = parts(parsed);
        let body = &parts["word/document.xml"];
        prop_assert!(!body.contains("SHARED TITLE") && !body.contains("SHARED FOOTER"));
        let sections: Vec<_> = body.split("<w:sectPr").skip(1)
            .map(|part| part.split("</w:sectPr>").next().unwrap()).collect();
        prop_assert_eq!(sections.len(), count);
        for (page, section) in sections.iter().enumerate() {
            for reference in [b"headerReference".as_slice(), b"footerReference".as_slice()] {
                let part = chrome_part(&parts, section, reference).unwrap();
                let size = if page % 2 == 0 { 18 } else { 20 };
                prop_assert!(part.contains(&format!("w:sz w:val=\"{size}\"")), "source page style mismatch");
            }
            prop_assert!(body.contains(&format!("PAGE{page} LEFT 1")), "source page body missing");
        }
    }
}

#[path = "page_chrome_origin_tests.rs"]
mod origins;
