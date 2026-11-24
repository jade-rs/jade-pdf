//! 狭い段間をまたぐ大きな文字へ、索引数字や別段の添字を対応させない。
use super::*;

/// 4行で確認できる19ptの段間と、二つの独立した文字サイズを持つ所有IRを作る。
fn column_source(local_formula: bool) -> ParsedDocument {
    let mut parsed = source(2, &[]);
    parsed.page_count = 1;
    parsed.pages.truncate(1);
    parsed.text_elements.clear();
    let mut add = |text: &str, left: f32, right: f32, y: f32, size: f32| {
        parsed.text_elements.push(ParsedTextElement {
            page: 0,
            text: text.into(),
            font_name: Some("Helvetica".into()),
            font_size: size,
            fill_color: None,
            baseline_y: Some(y),
            bounds: ParsedBounds {
                x0: left,
                x1: right,
                y0: y - size * 0.2,
                y1: y + size * 0.8,
            },
        });
    };
    for y in [720., 700., 680., 660.] {
        if local_formula && y == 700. {
            add("base", 50., 200., y, 9.);
            add("tail", 210., 296., y, 9.);
        } else {
            for index in 0..41 {
                let x = 50. + index as f32 * 6.;
                add("l", x, x + 6., y, 9.);
            }
        }
        for index in 0..39 {
            let x = 315. + index as f32 * 6.;
            add("r", x, x + 6., y, 9.);
        }
    }
    if local_formula {
        add("2", 201., 205., 704.5, 7.);
        add("H", 315., 325., 704.5, 12.);
    } else {
        for (index, text) in "entry..................................."
            .chars()
            .enumerate()
        {
            let x = 50. + index as f32 * 6.;
            add(&text.to_string(), x, x + 6., 690., 9.);
        }
        add("2", 292., 297., 690., 9.);
        add("H", 315., 325., 684.5, 12.);
    }
    parsed
}

/// 左の索引行の参照番号を、近くても右段の大きな見出しへ移さない。
#[test]
fn index_reference_does_not_attach_across_source_column_gutter() {
    let parsed = column_source(false);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    let entry = pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("entry"))
        .unwrap();
    assert!(
        entry.text.ends_with('2'),
        "existing index reference must not attach across the source column gutter"
    );
}

/// 別段の大きな文字と同じ高さでも、左段の添字は左段の通常文字を基準に復元する。
#[test]
fn local_script_uses_its_own_column_font_scale() {
    let parsed = column_source(true);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    let formula = pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("base"))
        .unwrap();
    assert!(
        formula.text.contains('2'),
        "the other column heading must not hide the local script's main font"
    );
}
