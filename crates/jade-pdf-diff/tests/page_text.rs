use pdf_diff::{diff_documents, DiffOptions, DocumentIr, PageIr, SegmentKind};
use proptest::prelude::*;
#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;

/// 行の分割だけが異なる、有限座標を持つ検証用IRを組み立てる。
fn page(lines: &[String]) -> PageIr {
    serde_json::from_value(serde_json::json!({
        "index": 1, "width": 612., "height": 792., "user_unit": 1., "rotation": 0,
        "sdk_unicode_mapping_errors": 0,
        "blocks": lines.iter().enumerate().map(|(i,text)| serde_json::json!({
            "index":i+1,"text":text,"size":10.,
            "bounds":{"x0":40.,"y0":700.-i as f64*14.,"x1":560.,"y1":710.-i as f64*14.}
        })).collect::<Vec<_>>()
    }))
    .unwrap()
}

/// 元の行ごとの文字列と座標を保持し、本文の折り返しを文字の削除・追加から区別する。
#[test]
fn page_text_retains_line_evidence_and_eliminates_wrap_only_edits() {
    let old = DocumentIr {
        pages: vec![page(&[
            "Header".into(),
            "If you".into(),
            "agree".into(),
            "End".into(),
        ])],
    };
    let new = DocumentIr {
        pages: vec![page(&[
            "Header".into(),
            "If".into(),
            "you agree".into(),
            "End".into(),
        ])],
    };
    let diff = diff_documents("old", "new", &old, &new, &DiffOptions::default());
    let view = diff.pages[0].text_diff.as_ref().unwrap();
    assert!(view.groups.is_empty());
    assert!(!view.alignment_limited);
    assert!(diff.pages[0]
        .blocks
        .iter()
        .any(|b| b.old_text != b.new_text));
    assert_eq!(
        diff.pages[0]
            .blocks
            .iter()
            .find(|b| b.old_index == Some(2))
            .unwrap()
            .old_text
            .as_deref(),
        Some("If you")
    );
}

/// 折り返しと同時に追加された語や、同じ語集合の順序変更は本文差分として保持する。
#[test]
fn page_text_preserves_actual_edits_order_and_reading_diagnostics() {
    let mut old = page(&[
        "Header".into(),
        "If you".into(),
        "agree".into(),
        "End".into(),
    ]);
    old.reading_order_requires_review = true;
    let new = page(&[
        "Header".into(),
        "If the".into(),
        "you agree".into(),
        "End".into(),
    ]);
    let diff = diff_documents(
        "old",
        "new",
        &DocumentIr { pages: vec![old] },
        &DocumentIr { pages: vec![new] },
        &DiffOptions::default(),
    );
    let view = diff.pages[0].text_diff.as_ref().unwrap();
    assert_eq!(view.groups.len(), 1);
    let inserted: String = view.groups[0]
        .segments
        .iter()
        .filter(|s| s.kind == SegmentKind::Insert)
        .map(|s| s.text.as_str())
        .collect();
    let deleted: String = view.groups[0]
        .segments
        .iter()
        .filter(|s| s.kind == SegmentKind::Delete)
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(inserted, "the ");
    assert!(deleted.is_empty());
    assert!(!diff.is_text_comparison_complete());
    let reversed = diff_documents(
        "a",
        "b",
        &DocumentIr {
            pages: vec![page(&["alpha".into(), "beta".into()])],
        },
        &DocumentIr {
            pages: vec![page(&["beta".into(), "alpha".into()])],
        },
        &DiffOptions::default(),
    );
    assert!(reversed
        .pages
        .iter()
        .all(|p| p.text_diff.as_ref().is_some_and(|v| !v.groups.is_empty())));
}

proptest! {
    /// 単語を落としたり並べ替えず、異なる行数への折り返しだけでは本文編集を作らない。
    #[test]
    fn page_text_is_invariant_under_word_boundary_wrapping(words in prop::collection::vec("[a-z]{1,8}", 1..40), old_width in 1usize..10, new_width in 1usize..10) {
        let old:Vec<_>=words.chunks(old_width).map(|part|part.join(" ")).collect();
        let new:Vec<_>=words.chunks(new_width).map(|part|part.join(" ")).collect();
        let options=DiffOptions{page_match_score:0.,..DiffOptions::default()};
        let diff=diff_documents("a","b",&DocumentIr{pages:vec![page(&old)]},&DocumentIr{pages:vec![page(&new)]},&options);
        prop_assert_eq!(diff.pages.len(),1);
        prop_assert!(diff.pages[0].text_diff.as_ref().unwrap().groups.is_empty());
    }
}

/// 予算超過を細かな本文比較の成功にせず、元の全文と要確認の理由を保持する。
#[test]
fn page_text_budget_is_explicit_and_legacy_json_remains_uncomputed() {
    let old = DocumentIr {
        pages: vec![page(&["Header".into(), "a".repeat(2100), "End".into()])],
    };
    let new = DocumentIr {
        pages: vec![page(&["Header".into(), "b".repeat(2100), "End".into()])],
    };
    let options = DiffOptions {
        page_match_score: 0.,
        ..DiffOptions::default()
    };
    let diff = diff_documents("a", "b", &old, &new, &options);
    assert_eq!(diff.pages.len(), 1);
    let view = diff.pages[0].text_diff.as_ref().unwrap();
    assert!(view.alignment_limited);
    assert_eq!(diff.diagnostics.page_text_alignment_limited_pages, 1);
    assert!(!diff.is_text_comparison_complete());
    let group = &view.groups[0];
    let old: String = group
        .segments
        .iter()
        .filter(|s| s.kind != SegmentKind::Insert)
        .map(|s| s.text.as_str())
        .collect();
    let new: String = group
        .segments
        .iter()
        .filter(|s| s.kind != SegmentKind::Delete)
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(old, group.old_text);
    assert_eq!(new, group.new_text);
    let mut legacy = serde_json::to_value(&diff).unwrap();
    legacy["pages"][0]
        .as_object_mut()
        .unwrap()
        .remove("text_diff");
    legacy["diagnostics"]
        .as_object_mut()
        .unwrap()
        .remove("page_text_alignment_limited_pages");
    let decoded: pdf_diff::DocumentDiff = serde_json::from_value(legacy).unwrap();
    assert!(decoded.pages[0].text_diff.is_none());
    assert_eq!(decoded.diagnostics.page_text_alignment_limited_pages, 0);
}

/// 実際のSDK経路でも単位・表示回転・描画順を跨ぎ、文字の編集と画像の移動を保持する。
#[test]
fn sdk_page_text_keeps_real_edits_and_visual_layout_changes() {
    for mode in ["wrap", "edit", "order", "repeat"] {
        for unit in [1. / 64., 1., 3., 64.] {
            for rotation in [0, 90, 180, 270] {
                for reverse in [false, true] {
                    let old = fixture::page_text_layout(mode, false, unit, rotation, reverse);
                    let new = fixture::page_text_layout(mode, true, unit, rotation, reverse);
                    let before =
                        pdf_diff::load_document_from_bytes(&old, &pdf_diff::Limits::default())
                            .unwrap();
                    let after =
                        pdf_diff::load_document_from_bytes(&new, &pdf_diff::Limits::default())
                            .unwrap();
                    let diff = diff_documents("a", "b", &before, &after, &DiffOptions::default());
                    assert_eq!(diff.pages.len(), 1);
                    let view = diff.pages[0].text_diff.as_ref().unwrap();
                    assert!(!view.alignment_limited);
                    assert_eq!(
                        view.groups.is_empty(),
                        matches!(mode, "wrap" | "repeat"),
                        "{mode} {unit} {rotation} {reverse}"
                    );
                    if unit >= 1. || matches!(mode, "edit" | "order") {
                        assert!(diff.has_changes(), "{mode} {unit} {rotation} {reverse}");
                    }
                    assert!(diff.is_text_comparison_complete());
                }
            }
        }
        let old = fixture::page_text_layout(mode, false, 1., 0, false);
        let new = fixture::page_text_layout(mode, true, 1., 0, false);
        let diff = pdf_diff::diff_bytes(
            "a",
            "b",
            &old,
            &new,
            &pdf_diff::Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            diff.is_comparison_complete(),
            "{mode}: {:?}",
            diff.diagnostics
        );
        assert_eq!(diff.summary.pages_visual_changed, 1);
        assert!(diff.has_changes());
    }
}

/// 位置の許容幅を増やしても、同じ単語の本文順序が変わった事実を無変更にしない。
#[test]
fn ordered_page_text_is_independent_of_position_tolerance() {
    let old = DocumentIr {
        pages: vec![page(&["alpha".into(), "beta".into()])],
    };
    let new = DocumentIr {
        pages: vec![page(&["beta".into(), "alpha".into()])],
    };
    let options = DiffOptions {
        position_tolerance: 20.,
        page_match_score: 0.,
        ..DiffOptions::default()
    };
    let diff = diff_documents("a", "b", &old, &new, &options);
    assert_eq!(diff.pages.len(), 1);
    assert!(!diff.pages[0].text_diff.as_ref().unwrap().groups.is_empty());
    assert!(diff.has_changes());
}
