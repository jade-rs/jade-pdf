//! Wordの本文余白を加味した実座標で、両端の見出しの基準位置を検証する。
use super::*;

/// 本文より左の行・中央行・右寄せ行を上下両方へ反復させる。
fn source_with_chrome(left: f32, right: f32) -> ParsedDocument {
    let mut parsed = source(2, &[]);
    parsed.text_elements.retain(|text| text.bounds.x0 == 48.);
    for text in &mut parsed.text_elements {
        text.bounds.x0 += 48.;
        text.bounds.x1 += 48.;
    }
    let template = parsed.text_elements[0].clone();
    for page in 0..2 {
        for (text, top) in [("RUNNING HEADER", 752.), ("RUNNING FOOTER", 40.)] {
            let mut line = template.clone();
            line.page = page;
            line.text = text.into();
            line.font_size = 9.;
            line.bounds = ParsedBounds {
                x0: left,
                x1: right,
                y0: top - 9.,
                y1: top,
            };
            line.baseline_y = Some(top - 9.);
            parsed.text_elements.push(line);
        }
    }
    parsed
}

/// XMLに存在する整数属性だけを読み、未指定時のWord既定値0を使う。
fn number(xml: &str, element: &[u8], key: &[u8]) -> f64 {
    attribute(xml, element, key)
        .map(|s| s.parse::<f64>().unwrap())
        .unwrap_or(0.)
}

/// Wordの左右余白・indent・整列の式からページ絶対座標のanchorを計算する。
fn word_anchor(part: &str, section: &str) -> f64 {
    let left = number(section, b"pgMar", b"w:left") + number(part, b"ind", b"w:left");
    let right = number(section, b"pgSz", b"w:w")
        - number(section, b"pgMar", b"w:right")
        - number(part, b"ind", b"w:right");
    match attribute(part, b"jc", b"w:val")
        .as_deref()
        .unwrap_or("left")
    {
        "center" => (left + right) / 2.,
        "right" => right,
        "left" => left,
        other => panic!("unexpected alignment {other}"),
    }
}

/// 左余白96ptを二重加算せず、見出しの中心・左端・右端を原本に合わせる。
#[test]
fn chrome_anchors_use_page_coordinates_instead_of_body_origin() {
    for (left, right, anchor) in [(190., 350., 270.), (32., 132., 32.), (520., 580., 580.)] {
        let parts = parts(source_with_chrome(left, right));
        let section = parts["word/document.xml"]
            .split("<w:sectPr")
            .nth(1)
            .unwrap()
            .split("</w:sectPr>")
            .next()
            .unwrap();
        assert_eq!(number(section, b"pgMar", b"w:left"), 1920.);
        for reference in [b"headerReference".as_slice(), b"footerReference".as_slice()] {
            let part = chrome_part(&parts, section, reference).unwrap();
            let actual = word_anchor(part, section) / 20.;
            assert!(
                (actual - anchor).abs() <= 0.025,
                "chrome anchor must match source coordinates: source={anchor}, Word={actual}"
            );
        }
    }
}
