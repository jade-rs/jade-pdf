//! SDKとsourceが同じ文字を示す場合の語間・variation sequenceの保持。
use super::*;
use proptest::prelude::*;

/// 文字内容以外のsource幾何・書体・weightを検査できる要素を作る。
fn element(text: &str, page: usize) -> ParsedTextElement {
    ParsedTextElement {
        text: text.into(),
        page,
        bounds: crate::ParsedBounds {
            x0: 20.,
            y0: 50.,
            x1: 90.,
            y1: 62.,
        },
        font_size: 12.,
        font_name: Some("OwnedFont".into()),
        font_weight: Some(700),
        fill_color: Some("123456".into()),
        baseline_y: Some(50.),
    }
}

/// 同一Unicodeのspan内でSDKが識別した数字間の語間は、source属性とともに残る。
#[test]
fn agreeing_unicode_keeps_native_numeric_word_boundaries() {
    let pure = vec![element("112", 0), element("720", 0)];
    let native = vec![element("1 12", 0), element("7 20", 0)];
    let mut expected = pure.clone();
    expected[0].text = "1 12".into();
    expected[1].text = "7 20".into();
    assert_eq!(
        reconcile(
            &DecodeBudget::default(),
            &pure,
            &native,
            &Default::default()
        ),
        Some(expected)
    );
}

/// SDKがselectorを省いても、同順のbase文字に対応するsourceのIVSを落とさない。
#[test]
fn agreeing_base_unicode_keeps_source_variation_sequence() {
    let pure = vec![element("芒\u{e0100}種 0\u{fe00}", 0)];
    let native = vec![element("芒種 0", 0)];
    assert_eq!(
        reconcile(
            &DecodeBudget::default(),
            &pure,
            &native,
            &Default::default()
        ),
        Some(pure)
    );
}

/// 明示空白・CJK tracking・生成fontのcode所有権には新しいSDK空白を足さない。
#[test]
fn explicit_spaces_cjk_tracking_and_generated_glyphs_remain_authoritative() {
    for (source, sdk, generated, expected_text) in [
        ("1 12", "112", false, "1 12"),
        ("1 12", "1 12", false, "1 12"),
        ("日本", "日 本", false, "日本"),
        ("112", "1 12", true, "112"),
        ("ABC", "1 12", false, "112"),
    ] {
        let mut pure = vec![element(source, 0)];
        if generated {
            pure[0].font_name = Some("JadeT1-verified".into());
        }
        let mut expected = pure.clone();
        expected[0].text = expected_text.into();
        assert_eq!(
            reconcile(
                &DecodeBudget::default(),
                &pure,
                &[element(sdk, 0)],
                &Default::default()
            ),
            Some(expected)
        );
    }
}

/// selectorを根拠に別文字や異なるページを対応付けず、生成fontへbaseだけを流さない。
#[test]
fn variation_sequence_never_hides_base_order_or_glyph_owner_mismatch() {
    for (source, sdk, generated) in [
        ("芒\u{e0100}種", "種芒", false),
        ("\u{e0100}芒種", "芒種", false),
        ("芒\u{e0100}\u{e0101}種", "芒種", false),
        ("芒\u{e0100}種", "芒種", true),
    ] {
        let mut pure = vec![element(source, 0)];
        if generated {
            pure[0].font_name = Some("JadeT1-verified".into());
        }
        let budget = DecodeBudget::default();
        assert!(reconcile(&budget, &pure, &[element(sdk, 0)], &Default::default()).is_none());
        assert!(budget.check().is_ok());
    }
}

/// 改行・tab・別spanの先頭を、同一span内の通常語間に読み替えない。
#[test]
fn line_boundaries_and_span_origins_do_not_gain_leading_spaces() {
    for sdk in ["1\n12", "1\n 12", "1\t12"] {
        let pure = vec![element("112", 0)];
        assert_eq!(
            reconcile(
                &DecodeBudget::default(),
                &pure,
                &[element(sdk, 0)],
                &Default::default()
            ),
            Some(pure)
        );
    }
    let pure = vec![element("1", 0), element("12", 0)];
    assert_eq!(
        reconcile(
            &DecodeBudget::default(),
            &pure,
            &[element("1 12", 0)],
            &Default::default()
        ),
        Some(pure)
    );
}

/// 同じspanに語間とIVSが共存しても、selectorはnative側の文字位置を進めない。
#[test]
fn word_boundaries_and_variation_sequences_share_the_same_alignment() {
    let pure = vec![element("112芒\u{e0100}種720", 0)];
    let native = vec![element("1 12芒種7 20", 0)];
    let mut expected = pure.clone();
    expected[0].text = "1 12芒\u{e0100}種7 20".into();
    assert_eq!(
        reconcile(
            &DecodeBudget::default(),
            &pure,
            &native,
            &Default::default()
        ),
        Some(expected)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// sourceの数字順・style・幾何を変えず、SDKで確認したspan内の全語間を一度だけ保持する。
    #[test]
    fn same_digits_retain_native_word_boundaries(words in prop::collection::vec("[1-9][0-9]{0,3}", 2..12)) {
        let source = words.concat(); let sdk = words.join(" "); let pure = vec![element(&source, 0)]; let native = vec![element(&sdk, 0)]; let mut expected = pure.clone(); expected[0].text = sdk;
        prop_assert_eq!(reconcile(&DecodeBudget::default(), &pure, &native, &Default::default()), Some(expected));
    }
}
