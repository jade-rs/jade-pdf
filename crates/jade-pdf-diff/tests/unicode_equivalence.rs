#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use pdf_diff::{diff_bytes, load_document_from_bytes, text_layer_from_bytes, DiffOptions, Limits};
use unicode_normalization::UnicodeNormalization;

/// 接続形と基底文字の表記差は原文欠落にせず、実際の文字変更は差分として残す。
#[test]
fn arabic_contextual_spelling_agrees_with_sdk_and_keeps_real_edits() {
    for (presentation, nominal, changed) in [
        ("\u{fe91}", "ب", "ت"),
        ("\u{fccb}", "لخ", "لح"),
        ("\u{fefb}", "لا", "لب"),
        ("\u{fdf2}", "الله", "اللة"),
    ] {
        let old = fixture::unicode_spelling(presentation);
        let same = fixture::unicode_spelling(nominal);
        let new = fixture::unicode_spelling(changed);
        for (source, expected) in [(&old, nominal), (&same, nominal), (&new, changed)] {
            let ir = load_document_from_bytes(source, &Limits::default()).unwrap();
            assert_eq!(
                ir.pages[0].source_unicode_scalars_unmatched, 0,
                "{presentation:?}"
            );
            assert!(!ir.pages[0].source_unicode_accounting_unavailable);
            assert_eq!(ir.pages[0].text(), expected, "{presentation:?}");
            let layer = text_layer_from_bytes(source, 1).unwrap();
            assert_eq!(
                layer
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                expected
            );
            assert!(layer
                .iter()
                .all(|run| run.source_unicode_scalars_unmatched == 0));
        }
        let equal = diff_bytes(
            "contextual",
            "nominal",
            &old,
            &same,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            equal.is_comparison_complete(),
            "{presentation:?}: {:?}",
            equal.diagnostics
        );
        assert!(
            !equal.has_changes(),
            "{presentation:?}: {:?}",
            equal.summary
        );
        let edited = diff_bytes(
            "contextual",
            "edited",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            edited.is_comparison_complete(),
            "{presentation:?}: {:?}",
            edited.diagnostics
        );
        assert!(edited.has_changes(), "{presentation:?}");
        assert_eq!(edited.summary.blocks_changed, 1, "{presentation:?}");
    }
}

/// 原文順序が候補の文字集合だけでは決まらない場合は、比較を確定しない。
#[test]
fn ambiguous_source_orders_keep_reading_review_in_all_text_routes() {
    let source = fixture::ambiguous_rtl_spellings();
    let mut rewrite = source.clone();
    rewrite.extend_from_slice(b"\n% equivalent rewrite\n");
    let ir = load_document_from_bytes(&source, &Limits::default()).unwrap();
    assert!(ir.pages[0].reading_order_requires_review);
    assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
    assert!(!ir.pages[0].source_unicode_accounting_unavailable);
    let layer = text_layer_from_bytes(&source, 1).unwrap();
    assert!(layer.iter().all(|run| run.reading_order_requires_review));
    let diff = diff_bytes(
        "a",
        "b",
        &source,
        &rewrite,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!diff.is_comparison_complete());
    assert!(!diff.has_changes());
}

/// 双方向制御・結合制御や改行の順序を、確証なく逆転して確定しない。
#[test]
fn mixed_rtl_glyph_expansions_remain_explicitly_reviewable() {
    for target in ["ب\u{200c}ت", "ب\u{202e}ت", "\u{202e}", "ب\nت", "ب𐐌"] {
        let source = fixture::unicode_spelling(target);
        let ir = load_document_from_bytes(&source, &Limits::default()).unwrap();
        assert!(ir.pages[0].reading_order_requires_review, "{target:?}");
        let mut rewrite = source.clone();
        rewrite.extend_from_slice(b"\n% equivalent rewrite\n");
        let diff = diff_bytes(
            "a",
            "b",
            &source,
            &rewrite,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(!diff.is_comparison_complete(), "{target:?}");
    }
}

/// 回転・物理単位・斜体行列を跨いでも原文順序と文字位置の対応を保つ。
#[test]
fn rtl_glyph_order_is_stable_across_display_units_and_shear() {
    for unit in [1.0, 3.0, 64.0] {
        for rotation in [0, 90, 180, 270] {
            for shear in [false, true] {
                let source = fixture::unicode_spelling_geometry("لخ", unit, rotation, shear);
                let ir = load_document_from_bytes(&source, &Limits::default()).unwrap();
                assert_eq!(ir.pages[0].text(), "لخ", "{unit} {rotation} {shear}");
                assert!(!ir.pages[0].reading_order_requires_review);
                let layer = text_layer_from_bytes(&source, 1).unwrap();
                assert_eq!(layer.len(), 1);
                assert_eq!(layer[0].text, "لخ");
                assert_eq!(layer[0].chars.len(), 2);
                assert_eq!(layer[0].chars[0].x0, layer[0].chars[1].x0);
                assert_eq!(layer[0].chars[0].x1, layer[0].chars[1].x1);
            }
        }
    }
}

/// 原文と同一セルから順序を検証できる混在文字・結合記号も復元する。
#[test]
fn source_owned_mixed_words_and_marks_keep_their_logical_order() {
    for target in ["بA", "ب1", "بَ", "ب ت", "بé", "שׁל"] {
        let source = fixture::unicode_spelling(target);
        let ir = load_document_from_bytes(&source, &Limits::default()).unwrap();
        assert_eq!(ir.pages[0].text(), target, "{target:?}");
        assert!(!ir.pages[0].reading_order_requires_review, "{target:?}");
        let layer = text_layer_from_bytes(&source, 1).unwrap();
        assert_eq!(
            layer
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            target
        );
        let mut rewrite = source.clone();
        rewrite.extend_from_slice(b"\n% equivalent rewrite\n");
        let diff = diff_bytes(
            "a",
            "b",
            &source,
            &rewrite,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            diff.is_comparison_complete(),
            "{target:?}: {:?}",
            diff.diagnostics
        );
        assert!(!diff.has_changes(), "{target:?}");
    }
}

/// 数字や欧文だけ正順で残る場合も、一意な原文と同一セルから論理順序を復元する。
#[test]
fn partially_reordered_mixed_glyphs_follow_the_unique_source_sequence() {
    for target in ["بABت", "ب12ت", "אב12גד", "ب123.45ت", "ب(12)ت", "ب/AB/ت"] {
        for unit in [1.0, 3.0, 64.0] {
            for rotation in [0, 90, 180, 270] {
                for shear in [false, true] {
                    let source = fixture::unicode_spelling_geometry(target, unit, rotation, shear);
                    let ir = load_document_from_bytes(&source, &Limits::default()).unwrap();
                    assert_eq!(
                        ir.pages[0].text(),
                        target,
                        "{target:?} {unit} {rotation} {shear}"
                    );
                    assert!(!ir.pages[0].reading_order_requires_review);
                    assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
                    let layer = text_layer_from_bytes(&source, 1).unwrap();
                    assert_eq!(layer.len(), 1);
                    assert_eq!(layer[0].text, target);
                    assert_eq!(layer[0].chars.len(), target.chars().count());
                    assert!(layer[0]
                        .chars
                        .iter()
                        .all(|cell| cell.x0 == layer[0].chars[0].x0
                            && cell.x1 == layer[0].chars[0].x1));
                }
            }
        }
    }
}

/// 同じ字形の字面でも実際の数字・文字・論理順序の編集を消さず、正準結合順だけを揃える。
#[test]
fn verified_permutations_keep_real_edits_and_canonical_marks() {
    for (before, after) in [("ب12ت", "ب13ت"), ("بABت", "بACت"), ("ب12ت", "ت12ب")] {
        let old = fixture::unicode_spelling(before);
        let new = fixture::unicode_spelling(after);
        let diff = diff_bytes(
            "before",
            "after",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            diff.is_comparison_complete(),
            "{before:?} {after:?}: {:?}",
            diff.diagnostics
        );
        assert_eq!(diff.summary.blocks_changed, 1);
        assert_eq!(diff.summary.pages_visual_changed, 0);
        assert!(diff.has_changes());
    }
    let target = "ب\u{651}\u{64e}ت";
    let source = fixture::unicode_spelling(target);
    let ir = load_document_from_bytes(&source, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text(), target.nfc().collect::<String>());
    assert!(!ir.pages[0].reading_order_requires_review);
    let layer = text_layer_from_bytes(&source, 1).unwrap();
    assert_eq!(
        layer[0].text.nfd().collect::<String>(),
        target.nfd().collect::<String>()
    );
    assert_eq!(layer[0].chars.len(), 4);
}
