//! 対応する相手行がない本文でも、実段組を行単位で往復しない回帰。
use super::*;
use proptest::prelude::*;

/// 上下二行だけ両段にあり、その間に片段だけの本文が続く所有入力。
fn uneven_source(extra_rows: usize, right_extra: bool) -> ParsedDocument {
    let mut parsed = source(2, &[]);
    for text in &mut parsed.text_elements {
        if text.text.ends_with("2") {
            text.bounds.y0 -= 95.;
            text.bounds.y1 -= 95.;
            text.baseline_y = text.baseline_y.map(|y| y - 95.);
        }
    }
    let template = parsed.text_elements[if right_extra { 2 } else { 0 }].clone();
    for page in 0..2 {
        for row in 0..extra_rows {
            let mut text = template.clone();
            text.page = page;
            text.bounds.y0 = 695. - (row + 1) as f32 * 120. / (extra_rows + 1) as f32;
            text.bounds.y1 = text.bounds.y0 + 11.;
            text.baseline_y = Some(text.bounds.y0);
            text.text = format!(
                "PAGE{} {} EXTRA{row}",
                char::from(b'A' + page as u8),
                if right_extra { "RIGHT" } else { "LEFT" }
            );
            parsed.text_elements.push(text);
        }
    }
    parsed
}

/// 段組の採用後は左段の全行から右段へ一度だけ移り、行ごとの改段でページを増やさない。
#[test]
fn uneven_column_lines_change_columns_once_per_source_page() {
    let (xml, _) = package_source(uneven_source(5, false));
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

proptest! {
    /// 相手のない行の数・左右が変わっても、本文を欠落させず各ページで一度だけ改段する。
    #[test]
    fn unpaired_row_counts_preserve_column_order(extra_rows in 1usize..=5, right_extra in any::<bool>()) {
        let (xml, _) = package_source(uneven_source(extra_rows, right_extra));
        prop_assert_eq!(xml.matches("w:num=\"2\"").count(), 2);
        prop_assert_eq!(xml.matches("w:type=\"column\"").count(), 2);
        for page in ['A','B'] {
            prop_assert!(xml.find(&format!("PAGE{page} LEFT 2")).unwrap() < xml.find(&format!("PAGE{page} RIGHT 1")).unwrap(), "left column must finish before right column starts");
            for row in 0..extra_rows {
                let label = format!("PAGE{page} {} EXTRA{row}", if right_extra { "RIGHT" } else { "LEFT" });
                prop_assert_eq!(xml.matches(&label).count(), 1);
            }
        }
    }
}
