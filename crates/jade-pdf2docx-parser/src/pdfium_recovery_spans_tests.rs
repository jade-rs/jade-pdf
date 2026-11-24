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
fn style_and_baseline_boundaries_remain_distinct() {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let source = element("AA", 10., 22.);
    let adjacent = element("AA", 22., 34.);
    let mut variants = vec![adjacent.clone(); 7];
    variants[0].page = 1;
    variants[1].baseline_y = Some(91.);
    variants[2].baseline_y = None;
    variants[3].font_name = Some("OtherFont".into());
    variants[4].fill_color = Some("FF0000".into());
    variants[5].font_size = 11.;
    variants[6].bounds.x0 = f32::NAN;
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
    budget.charge_metadata_workspace_bytes(u64::MAX);
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

/// 同じ字面が別位置にあるだけでは結合せず、全文・幾何が一致する元矩形だけを復元する。
#[test]
fn only_complete_original_rectangle_spans_are_retained() {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let a = element("A", 10., 22.);
    let b = element("B", 22., 34.);
    let key = Some(Key {
        geometry: [10., 90., 34., 102., 1., 0., 0., 1., 0., 0.],
    });
    let keys = [(key, false), (key, true)];
    let original = element("AB", 10., 34.);
    let output = retain(
        &budget,
        vec![a.clone(), b.clone()],
        &keys,
        std::slice::from_ref(&original),
    )
    .unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].text, "AB");
    assert_eq!(output[0].bounds.x1, 34.);
    for source in [
        element("ABC", 10., 34.),
        element("AB", 50., 74.),
        element("AA", 10., 34.),
    ] {
        let output = retain(&budget, vec![a.clone(), b.clone()], &keys, &[source]).unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].text, "A");
        assert_eq!(output[1].text, "B");
    }
    let output = retain(&budget, vec![a.clone(), a], &keys, &[original]).unwrap();
    assert_eq!(
        output.len(),
        2,
        "Unverified overpaint must remain separate indexed glyphs"
    );
}
