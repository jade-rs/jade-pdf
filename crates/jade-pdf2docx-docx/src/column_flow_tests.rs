//! 対応する相手行がない本文でも、実段組を行単位で往復しない回帰。
use super::*;

/// 上下二行だけ両段にあり、その間に左段だけの本文が続く所有入力。
fn uneven_source() -> ParsedDocument {
    let mut parsed = source(2, &[]);
    for text in &mut parsed.text_elements {
        if text.text.ends_with("2") {
            text.bounds.y0 -= 95.;
            text.bounds.y1 -= 95.;
            text.baseline_y = text.baseline_y.map(|y| y - 95.);
        }
    }
    let template = parsed.text_elements[0].clone();
    for page in 0..2 {
        for row in 0..5 {
            let mut text = template.clone();
            text.page = page;
            text.bounds.y0 = 675. - row as f32 * 20.;
            text.bounds.y1 = text.bounds.y0 + 11.;
            text.baseline_y = Some(text.bounds.y0);
            text.text = format!("PAGE{} LEFT EXTRA{row}", char::from(b'A' + page as u8));
            parsed.text_elements.push(text);
        }
    }
    parsed
}

/// 段組の採用後は左段の全行から右段へ一度だけ移り、行ごとの改段でページを増やさない。
#[test]
fn uneven_column_lines_change_columns_once_per_source_page() {
    let (xml, _) = package_source(uneven_source());
    assert_eq!(xml.matches("w:num=\"2\"").count(), 2);
    assert_eq!(
        xml.matches("w:type=\"column\"").count(),
        2,
        "real two-column sections must switch columns only once per source page"
    );
    for page in ['A', 'B'] {
        assert!(
            xml.find(&format!("PAGE{page} LEFT 2")).unwrap()
                < xml.find(&format!("PAGE{page} RIGHT 1")).unwrap()
        );
    }
}
