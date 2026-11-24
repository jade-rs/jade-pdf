//! span結合の文字保持・境界分離・停止予算を確認する。
use super::*;
use crate::{ParseLimits, ParsedBounds};

/// 独立した幾何のtest用fingerprintを作る。
fn fingerprint(offset: f32) -> Option<Key> {
    Some(Key {
        geometry: [offset; 10],
    })
}

/// 水平な同一fontの連続文字を作る。
fn element(text: &str, left: f32, right: f32) -> ParsedTextElement {
    ParsedTextElement {
        text: text.into(),
        page: 0,
        bounds: ParsedBounds {
            x0: left,
            y0: 90.,
            x1: right,
            y1: 102.,
        },
        baseline_y: Some(92.),
        font_size: 12.,
        font_name: Some("OwnedFont".into()),
        fill_color: Some("000000".into()),
    }
}

/// 同じ文字列でもobject・paint・baseline・pageが異なれば結合しない。
#[test]
fn boundaries_and_overpaint_remain_distinct() {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let source = element("AA", 10., 22.);
    let adjacent = element("AA", 22., 34.);
    let mut variants = vec![adjacent.clone(); 9];
    variants[0].page = 1;
    variants[1].baseline_y = Some(91.);
    variants[2].baseline_y = None;
    variants[3].font_name = Some("OtherFont".into());
    variants[4].fill_color = Some("FF0000".into());
    variants[5].font_size = 11.;
    variants[6].bounds = source.bounds; // 実際の重ね描き。
    variants[7].bounds.x0 = 50.;
    variants[7].bounds.x1 = 62.; // 離れたcolumn。
    variants[8].bounds.x0 = f32::NAN;
    for next in variants {
        let mut previous = source.clone();
        assert!(
            !join(
                &budget,
                &mut previous,
                &next,
                fingerprint(1.),
                fingerprint(1.)
            )
            .unwrap()
        );
        assert_eq!(previous.text, "AA");
    }
    for (a, b) in [
        (None, None),
        (fingerprint(1.), None),
        (fingerprint(1.), fingerprint(2.)),
    ] {
        let mut previous = source.clone();
        assert!(!join(&budget, &mut previous, &adjacent, a, b).unwrap());
        assert_eq!(previous.text, "AA");
    }
}

/// 停止済みの予算では、結合の成否にかかわらず部分結果を返さない。
#[test]
fn stopped_budget_never_returns_a_partial_span() {
    let budget = DecodeBudget::new(&ParseLimits::default());
    budget.charge_metadata_workspace_bytes(usize::MAX);
    let mut previous = element("A", 10., 16.);
    assert!(
        join(
            &budget,
            &mut previous,
            &element("B", 16., 22.),
            fingerprint(1.),
            fingerprint(1.)
        )
        .is_err()
    );
    assert_eq!(previous.text, "A");
}

proptest::proptest! {
    /// 原点と文字幅・反復数を変えても、各文字を一回保持し外接矩形はunionと一致する。
    #[test]
    fn joining_preserves_unicode_repetitions_and_union(
        origin in -1000i16..1000, width in 3u16..9, count in 2usize..80,
    ) {
        let budget = DecodeBudget::new(&ParseLimits::default());
        let x = f32::from(origin);
        let w = f32::from(width);
        let mut output = element("一😀A", x, x + w);
        for index in 1..count {
            let next = element("一😀A", x + index as f32 * w, x + (index + 1) as f32 * w);
            proptest::prop_assert!(join(&budget, &mut output, &next, fingerprint(1.), fingerprint(1.)).unwrap());
        }
        proptest::prop_assert_eq!(output.text, "一😀A".repeat(count));
        proptest::prop_assert_eq!(output.bounds.x0, x);
        proptest::prop_assert_eq!(output.bounds.x1, x + count as f32 * w);
    }
}
