//! 同じ行の文字を分割しても、既存の下線を失わない幾何・装飾回帰。
use super::{ParsedBounds, ParsedGraphicElement, ParsedTextElement, structured_text_runs_from_row};
use proptest::prelude::*;

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

/// 行幅を共有しても、罫線から離れた別の語や別ページへ下線を広げない。
#[test]
fn underline_context_does_not_decorate_other_words_or_pages() {
    let first = heading("Heading", 40., 124.);
    let other = heading("Other", 400., 460.);
    let mut rule = ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 0.,
            y0: 98.,
            x1: 300.,
            y1: 98.,
        },
        stroke_width: 1.,
    };
    let runs = structured_text_runs_from_row(&[&first, &other], &[rule.clone()]);
    assert!(runs[0].style.underlined);
    assert!(!runs[1].style.underlined);
    rule.page = 1;
    assert!(
        structured_text_runs_from_row(&[&first, &other], &[rule])
            .iter()
            .all(|run| !run.style.underlined)
    );
}

proptest! {
    /// 原点・font高・分割位置が変わっても、一つの行の同じ罫線対応を保持する。
    #[test]
    fn underline_survives_row_fragmentation_at_different_scales(
        x in 20u16..80, baseline in 50u16..700, size in 6u16..24, split in 1usize..7,
    ) {
        let (x, y, size) = (f32::from(x), f32::from(baseline), f32::from(size));
        let advance = size * 0.6;
        let width = advance * 7.;
        let make = |text: &str, offset: usize, count: usize| {
            let mut element = heading(text, x + offset as f32 * advance, x + (offset + count) as f32 * advance);
            element.bounds.y0 = y;
            element.bounds.y1 = y + size;
            element.baseline_y = Some(y);
            element.font_size = size;
            element
        };
        let whole = make("Heading", 0, 7);
        let left = make(&whole.text[..split], 0, split);
        let right = make(&whole.text[split..], split, 7 - split);
        let rule = ParsedGraphicElement {
            page: 0,
            bounds: ParsedBounds { x0: x - size, y0: y - size / 6., x1: x + width * 3., y1: y - size / 6. },
            stroke_width: 1.,
        };
        let expected = structured_text_runs_from_row(&[&whole], &[rule.clone()]);
        let actual = structured_text_runs_from_row(&[&left, &right], &[rule]);
        prop_assert!(expected.iter().all(|run| run.style.underlined));
        prop_assert!(actual.iter().all(|run| run.style.underlined));
        prop_assert_eq!(actual.iter().map(|run| run.text.as_str()).collect::<String>(), "Heading");
    }
}
