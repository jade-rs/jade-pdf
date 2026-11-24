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
