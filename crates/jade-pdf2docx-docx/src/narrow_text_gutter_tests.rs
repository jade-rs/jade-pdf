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
