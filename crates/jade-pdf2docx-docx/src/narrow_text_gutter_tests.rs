//! 複数行で継続する狭い段間を、単語間の空白と混同せず段組として復元する。
use super::*;

/// 原本の字面幅が235ptで段間20ptの、自作二段本文の解析結果を作る。
fn narrow_source() -> ParsedDocument {
    let mut parsed = source(2, &[]);
    let template = parsed.text_elements[0].clone();
    parsed.text_elements.clear();
    for page in 0..2 {
        for (side, left) in [("LEFT", 54.), ("RIGHT", 309.)] {
            for row in 0..8 {
                let mut text = template.clone();
                text.page = page;
                text.font_size = 10.;
                text.bounds = ParsedBounds {
                    x0: left,
                    x1: left + 235.,
                    y0: 700. - row as f32 * 18.,
                    y1: 710. - row as f32 * 18.,
                };
                text.baseline_y = Some(text.bounds.y0);
                text.text = format!("SOURCE PAGE{page} {side} ROW{row} keeps its original words.");
                parsed.text_elements.push(text);
            }
        }
    }
    parsed
}

/// 8字高より狭い段間でも、同じ空白帯を複数行が共有すると左右本文を混ぜない。
#[test]
fn sustained_narrow_gutter_preserves_separate_columns() {
    let (xml, _) = package_source(narrow_source());
    assert_eq!(
        xml.matches("w:num=\"2\"").count(),
        2,
        "sustained narrow gutter must preserve separate columns"
    );
    for page in 0..2 {
        for side in ["LEFT", "RIGHT"] {
            for row in 0..8 {
                assert_eq!(
                    xml.matches(&format!("SOURCE PAGE{page} {side} ROW{row}"))
                        .count(),
                    1
                );
            }
        }
    }
}

/// 実際に保存したWordの左段領域が右段の原本字面へ侵入しないことを確認する。
#[test]
fn narrow_column_frames_do_not_clip_original_right_text() {
    use quick_xml::events::Event;
    let (xml, _) = package_source(narrow_source());
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut width = 0.;
    let mut left = 0.;
    let mut right = 0.;
    let mut gap = 0.;
    let mut count = 1;
    let mut first_width = None;
    let mut checked = 0;
    loop {
        match reader.read_event().unwrap() {
            Event::Empty(e) | Event::Start(e)
                if matches!(
                    e.local_name().as_ref(),
                    b"pgSz" | b"pgMar" | b"cols" | b"col"
                ) =>
            {
                let attrs = e
                    .attributes()
                    .map(Result::unwrap)
                    .map(|a| {
                        (
                            String::from_utf8(a.key.local_name().as_ref().to_vec()).unwrap(),
                            String::from_utf8(a.value.to_vec()).unwrap(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let value = |name: &str| attrs.get(name).unwrap().parse::<f64>().unwrap();
                match e.local_name().as_ref() {
                    b"pgSz" => width = value("w"),
                    b"pgMar" => {
                        left = value("left");
                        right = value("right");
                    }
                    b"cols" => {
                        count = value("num") as usize;
                        gap = value("space");
                        first_width = None;
                    }
                    b"col" if first_width.is_none() => first_width = Some(value("w")),
                    _ => (),
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"sectPr" && count == 2 => {
                let first_end =
                    (left + first_width.unwrap_or((width - left - right - gap) / 2.)) / 20.;
                assert!(
                    first_end < 309.,
                    "left Word column frame must end before original right text: end={first_end}, source=309"
                );
                checked += 1;
            }
            Event::Eof => break,
            _ => (),
        }
    }
    assert_eq!(checked, 2);
}

/// 単一・複数ページと全幅の前後行が混在しても、保存段幅とindentから原本位置へ戻れる。
#[test]
fn saved_column_widths_and_indents_preserve_each_source_page() {
    use quick_xml::events::Event;
    for single_page in [true, false] {
        let mut parsed = narrow_source();
        if single_page {
            parsed.page_count = 1;
            parsed.pages.truncate(1);
            parsed.text_elements.retain(|text| text.page == 0);
        }
        // 2ページ目は既存の均等段が安全な位置に変え、明示段幅との混在も確認する。
        for text in &mut parsed.text_elements {
            if text.page == 1 && text.bounds.x0 > 300. {
                text.bounds.x0 += 32.;
                text.bounds.x1 += 32.;
            }
        }
        let template = parsed.text_elements[0].clone();
        for (label, y) in [("FULL WIDTH HEADING", 740.), ("FULL WIDTH TRAILER", 450.)] {
            let mut text = template.clone();
            text.text = label.into();
            text.baseline_y = Some(y);
            text.bounds = ParsedBounds {
                x0: 54.,
                x1: 544.,
                y0: y,
                y1: y + 10.,
            };
            parsed.text_elements.push(text);
        }
        let (xml, _) = package_source(parsed);
        let mut reader = quick_xml::Reader::from_str(&xml);
        let mut width = 0.;
        let mut left = 0.;
        let mut right = 0.;
        let mut count = 1;
        let mut gap = 0.;
        let mut first_width = None;
        let mut paragraphs = Vec::new();
        let mut text = String::new();
        let mut indent = 0.;
        let mut right_labels = 0;
        let mut in_text = false;
        let mut in_paragraph = false;
        let mut boundary = false;
        loop {
            match reader.read_event().unwrap() {
                Event::Start(e) if e.name().as_ref() == b"w:p" => {
                    text.clear();
                    indent = 0.;
                    in_paragraph = true;
                }
                Event::Start(e) if e.name().as_ref() == b"w:t" => in_text = true,
                Event::End(e) if e.name().as_ref() == b"w:t" => in_text = false,
                Event::Text(e) if in_text => {
                    text.push_str(std::str::from_utf8(e.as_ref()).unwrap())
                }
                Event::Empty(e) | Event::Start(e)
                    if matches!(
                        e.local_name().as_ref(),
                        b"pgSz" | b"pgMar" | b"cols" | b"col" | b"ind"
                    ) =>
                {
                    let a = e
                        .attributes()
                        .map(Result::unwrap)
                        .map(|a| {
                            (
                                String::from_utf8(a.key.local_name().as_ref().to_vec()).unwrap(),
                                String::from_utf8(a.value.to_vec()).unwrap(),
                            )
                        })
                        .collect::<BTreeMap<_, _>>();
                    let number = |name: &str| a.get(name).unwrap().parse::<i32>().unwrap();
                    match e.local_name().as_ref() {
                        b"pgSz" => width = f64::from(number("w")),
                        b"pgMar" => {
                            left = f64::from(number("left"));
                            right = f64::from(number("right"));
                        }
                        b"cols" => {
                            count = number("num") as usize;
                            gap = f64::from(number("space"));
                            first_width = None;
                        }
                        b"col" if first_width.is_none() => {
                            first_width = Some(f64::from(number("w")))
                        }
                        b"ind" => {
                            indent = a
                                .get("left")
                                .map(|value| value.parse::<f64>().unwrap())
                                .unwrap_or(0.)
                        }
                        _ => (),
                    }
                }
                Event::End(e) if e.name().as_ref() == b"w:p" => {
                    paragraphs.push((std::mem::take(&mut text), indent));
                    in_paragraph = false;
                }
                Event::End(e) if e.name().as_ref() == b"w:sectPr" => boundary = true,
                Event::Eof => break,
                _ => (),
            }
            // pPrのsectPrは本文runより先に現れるため、境界段落の終端まで所属を確定しない。
            if boundary && !in_paragraph {
                if count == 2 {
                    let first = first_width.unwrap_or((width - left - right - gap) / 2.);
                    for (text, indent) in &paragraphs {
                        if text.contains("RIGHT") {
                            let expected = if text.contains("PAGE0") { 309. } else { 341. };
                            let actual = (left + first + gap + indent) / 20.;
                            assert!(
                                (actual - expected).abs() <= 0.0251,
                                "saved frame/indent must recover original x: {text}: {actual} != {expected}"
                            );
                            right_labels += 1;
                        }
                    }
                }
                paragraphs.clear();
                boundary = false;
            }
        }
        assert_eq!(right_labels, if single_page { 8 } else { 16 });
        assert_eq!(xml.matches("FULL WIDTH HEADING").count(), 1);
        assert_eq!(xml.matches("FULL WIDTH TRAILER").count(), 1);
    }
}
