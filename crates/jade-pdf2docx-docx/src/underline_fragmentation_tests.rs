//! 同じ行の文字を分割しても、既存の下線を失わない幾何・装飾回帰。
use super::{ParsedBounds, ParsedGraphicElement, ParsedTextElement, structured_text_runs_from_row};

/// 本文・範囲だけを変え、同じ水平baselineとstyleを持つ所有要素を作る。
fn heading(text: &str, left: f32, right: f32) -> ParsedTextElement {
    ParsedTextElement {
        page: 0,
        bounds: ParsedBounds {
            x0: left,
            y0: 100.,
            x1: right,
            y1: 112.,
        },
        baseline_y: Some(100.),
        font_size: 12.,
        font_name: Some("Helvetica".into()),
        fill_color: Some("000000".into()),
        text: text.into(),
    }
}

/// 元の本文・座標・罫線が同じなら、文字の抽出単位だけで見出し下線を失わない。
#[test]
fn splitting_a_heading_into_glyphs_retains_its_underline() {
    let rule = ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 0.,
            y0: 98.,
            x1: 300.,
            y1: 98.,
        },
        stroke_width: 1.,
    };
    let whole = heading("Heading", 40., 124.);
    let expected = structured_text_runs_from_row(&[&whole], &[rule.clone()]);
    assert_eq!(expected.len(), 1);
    assert!(expected[0].style.underlined);
    let glyphs = "Heading"
        .chars()
        .enumerate()
        .map(|(i, ch)| heading(&ch.to_string(), 40. + i as f32 * 12., 52. + i as f32 * 12.))
        .collect::<Vec<_>>();
    let actual = structured_text_runs_from_row(&glyphs.iter().collect::<Vec<_>>(), &[rule]);
    assert_eq!(
        actual
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "Heading"
    );
    assert!(
        actual.iter().all(|run| run.style.underlined),
        "fragmenting a row must preserve its existing underline"
    );
}
