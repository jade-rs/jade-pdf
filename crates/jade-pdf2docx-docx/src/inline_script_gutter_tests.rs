//! 外れた添字の復元が、本文行の水平分割を変えない回帰。
use super::*;
use proptest::prelude::*;

/// 四つの同じ大きさの本文断片と、任意の離れた添字を持つ所有IRを作る。
fn row_source(script: bool, gap: f32, size: f32, delta: f32, after_symbol: bool) -> ParsedDocument {
    let mut parsed = source(2, &[]);
    parsed.page_count = 1;
    parsed.pages.truncate(1);
    parsed.text_elements = [
        ("10", 50., 68., 700., 18.),
        ("prefix", 150., 180., 700., 18.),
        ("Q", 250., 262., 700., 18.),
        ("100000", 262. + gap, 352. + gap, 700., 18.),
        ("Next", 50., 120., 680., 18.),
    ]
    .into_iter()
    .map(|(text, left, right, y, font_size)| ParsedTextElement {
        source_background: None,
        fill_stroke_width: None,
        font_weight: None,
        page: 0,
        text: text.into(),
        font_name: Some("Helvetica".into()),
        font_size,
        fill_color: None,
        baseline_y: Some(y),
        bounds: ParsedBounds {
            x0: left,
            x1: right,
            y0: y,
            y1: y + font_size,
        },
    })
    .collect();
    if script {
        let mut exponent = parsed.text_elements[0].clone();
        exponent.text = "2".into();
        exponent.font_size = size;
        exponent.baseline_y = Some(700. + delta);
        exponent.bounds = ParsedBounds {
            x0: if after_symbol { 262. } else { 68. },
            x1: if after_symbol { 269. } else { 75. },
            y0: 700. + delta,
            y1: 700. + delta + size,
        };
        parsed.text_elements.push(exponent);
    }
    parsed
}

/// 本文の140pt間隔は、復元した小さい指数で別の段へ変わらない。
#[test]
fn recovered_script_does_not_detach_the_existing_value_column() {
    let original =
        text_lines_by_page_from_pdf_text_elements(&row_source(false, 140., 10.5, 6., false), &[]);
    assert_eq!(original[0].lines.len(), 2);
    let recovered =
        text_lines_by_page_from_pdf_text_elements(&row_source(true, 140., 10.5, 6., false), &[]);
    let body = recovered[0]
        .lines
        .iter()
        .find(|l| l.text.contains("prefix"))
        .unwrap();
    assert!(
        body.text.contains("100000"),
        "recovered exponent must not detach the existing numeric column"
    );
    assert_eq!(recovered[0].lines.len(), original[0].lines.len());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 真の分割の前後の間隔でも、添字の大小・上下は本文の分割結果を変えない。
    #[test]
    fn outlying_script_preserves_body_column_cuts(
        gap in 110_u16..=190, size in 9_u8..=14, half_points in 9_u8..=18, below in any::<bool>(), after_symbol in any::<bool>(),
    ) {
        let delta=f32::from(half_points)/2.*if below {-1.} else {1.};
        let original=text_lines_by_page_from_pdf_text_elements(&row_source(false,f32::from(gap),f32::from(size),delta,after_symbol),&[]);
        let recovered=text_lines_by_page_from_pdf_text_elements(&row_source(true,f32::from(gap),f32::from(size),delta,after_symbol),&[]);
        let before=original[0].lines.iter().map(|l|l.text.chars().filter(|c|!c.is_whitespace()).collect::<String>()).collect::<Vec<_>>();
        let after=recovered[0].lines.iter().map(|l|l.text.chars().filter(|c|!c.is_whitespace()&&*c!='2').collect::<String>()).collect::<Vec<_>>();
        prop_assert_eq!(before,after);
    }
}

/// 元の広い段間は、左段の末尾へ指数を戻しても閉じない。
#[test]
fn recovered_script_does_not_close_an_existing_column_cut() {
    let original =
        text_lines_by_page_from_pdf_text_elements(&row_source(false, 150., 10.5, 6., true), &[]);
    assert_eq!(original[0].lines.len(), 3);
    let recovered =
        text_lines_by_page_from_pdf_text_elements(&row_source(true, 150., 10.5, 6., true), &[]);
    assert_eq!(
        recovered[0].lines.len(),
        3,
        "recovered script must preserve existing wide column cut"
    );
}
