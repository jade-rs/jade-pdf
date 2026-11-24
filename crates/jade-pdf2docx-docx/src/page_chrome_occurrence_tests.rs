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
        .filter(|name| *name == "word/document.xml" || name.starts_with("word/header"))
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
