#[path = "support/appearance_fixture.rs"]
mod fixture;

use pdf_diff::{
    diff_bytes, load_document, load_document_from_bytes, text_layer, text_layer_from_bytes,
    DiffOptions, Limits,
};

#[test]
fn unrepresented_objects_require_review_even_on_mixed_or_nested_pages() {
    for unit in [1.0, 64.0] {
        for rotation in [0, 90, 180, 270] {
            for nesting in [0, 2] {
                for mixed in [false, true] {
                    let old =
                        fixture::unobserved_text(unit, rotation, mixed, nesting, false, false);
                    let new = fixture::unobserved_text(unit, rotation, mixed, nesting, false, true);
                    let temp = tempfile::tempdir().unwrap();
                    let file = temp.path().join("source.pdf");
                    std::fs::write(&file, &old).unwrap();
                    let ir = load_document_from_bytes(&old, &Limits::default()).unwrap();
                    assert_eq!(
                        serde_json::to_value(&ir).unwrap(),
                        serde_json::to_value(load_document(&file, &Limits::default()).unwrap())
                            .unwrap()
                    );
                    let page = &ir.pages[0];
                    assert_eq!(page.sdk_text_objects_without_characters, 1);
                    assert_eq!(page.sdk_unicode_mapping_errors, 0);
                    assert!(!page.reading_order_requires_review);
                    assert_eq!(page.text(), if mixed { "C" } else { "" });
                    for layer in [text_layer(&file, 1), text_layer_from_bytes(&old, 1)] {
                        if mixed {
                            let runs = layer.unwrap();
                            assert_eq!(runs.len(), 1);
                            assert_eq!(runs[0].text, "C");
                            assert_eq!(runs[0].sdk_text_objects_without_characters, 1);
                        } else {
                            assert!(layer
                                .unwrap_err()
                                .to_string()
                                .contains("no character records"));
                        }
                    }
                    let diff = diff_bytes(
                        "a",
                        "b",
                        &old,
                        &new,
                        &Limits::default(),
                        &DiffOptions::default(),
                    )
                    .unwrap();
                    assert!(!diff.is_comparison_complete());
                    assert!(!diff.is_text_comparison_complete());
                    assert!(diff.diagnostics.has_unobserved_text_objects());
                    assert_eq!(diff.diagnostics.old_pages_with_unobserved_text_objects, 1);
                    assert_eq!(diff.diagnostics.new_pages_with_unobserved_text_objects, 1);
                    assert!(!diff.has_changes()); // Equal white pixels cannot certify logical equality.
                    let identity = diff_bytes(
                        "a",
                        "a",
                        &old,
                        &old,
                        &Limits::default(),
                        &DiffOptions::default(),
                    )
                    .unwrap();
                    assert!(identity.is_comparison_complete());
                    assert!(!identity.is_text_comparison_complete());
                }
            }
        }
    }
}

#[test]
fn empty_object_api_text_is_not_an_independent_whitespace_proof() {
    let old = fixture::fixture("unobserved_whitespace", false);
    let new = fixture::fixture("unobserved_whitespace", true);
    let result = diff_bytes(
        "a",
        "b",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!result.is_comparison_complete());
    assert!(result.diagnostics.has_unobserved_text_objects());
    let blank = fixture::fixture("image_only", false);
    assert!(text_layer_from_bytes(&blank, 1).unwrap().is_empty());
    assert_eq!(
        load_document_from_bytes(&blank, &Limits::default())
            .unwrap()
            .pages[0]
            .sdk_text_objects_without_characters,
        0
    );
}

#[test]
fn deeply_nested_text_objects_fail_explicitly_and_engine_recovers() {
    let bytes = fixture::unobserved_text(1.0, 0, true, 34, false, false);
    for error in [
        load_document_from_bytes(&bytes, &Limits::default()).unwrap_err(),
        text_layer_from_bytes(&bytes, 1).unwrap_err(),
    ] {
        assert!(
            error.to_string().contains("nesting exceeds work budget"),
            "{error:#}"
        );
    }
    let valid = fixture::fixture("unobserved_mixed", false);
    assert_eq!(
        load_document_from_bytes(&valid, &Limits::default())
            .unwrap()
            .pages[0]
            .text(),
        "C"
    );
}

#[test]
fn partial_source_omissions_gate_all_public_text_routes() {
    for (unit, rotation, nesting, mode) in [
        (1.0, 0, 0, 3),
        (1.0, 90, 0, 3),
        (64.0, 270, 0, 3),
        (1.0, 180, 2, 3),
        (1.0, 0, 0, 0),
        (1.0, 0, 0, 7),
    ] {
        let old = fixture::source_duplicate(unit, rotation, nesting, mode, "TJ", false);
        let new = fixture::source_duplicate(unit, rotation, nesting, mode, "TJ", true);
        let limits = Limits::default();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("new.pdf");
        std::fs::write(&path, &new).unwrap();
        let ir = load_document_from_bytes(&new, &limits).unwrap();
        let page = &ir.pages[0];
        assert_eq!(page.text(), "C");
        assert_eq!(page.sdk_text_objects_without_characters, 0);
        assert_eq!(page.source_text_characters_missing, 1);
        assert!(!page.source_text_accounting_unavailable);
        assert_eq!(
            serde_json::to_value(&ir).unwrap(),
            serde_json::to_value(load_document(&path, &limits).unwrap()).unwrap()
        );
        for layer in [text_layer(&path, 1), text_layer_from_bytes(&new, 1)] {
            let runs = layer.unwrap();
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].source_text_characters_missing, 1);
            assert!(!runs[0].source_text_accounting_unavailable);
        }
        let result =
            diff_bytes("old", "new", &old, &new, &limits, &DiffOptions::default()).unwrap();
        assert!(!result.is_comparison_complete());
        assert!(!result.is_text_comparison_complete());
        assert!(result.diagnostics.has_incomplete_source_text());
        assert_eq!(
            result.diagnostics.old_pages_with_missing_source_characters,
            0
        );
        assert_eq!(
            result.diagnostics.new_pages_with_missing_source_characters,
            1
        );
        let old_path = temp.path().join("old.pdf");
        std::fs::write(&old_path, &old).unwrap();
        let files =
            pdf_diff::diff_files(&old_path, &path, &limits, &DiffOptions::default()).unwrap();
        assert_eq!(
            serde_json::to_value(&files.diagnostics).unwrap(),
            serde_json::to_value(&result.diagnostics).unwrap()
        );
        let decoded: pdf_diff::DocumentIr =
            serde_json::from_value(serde_json::to_value(&ir).unwrap()).unwrap();
        assert_eq!(decoded.pages[0].source_text_characters_missing, 1);
        let pure = pdf_diff::diff_documents("old", "new", &ir, &ir, &DiffOptions::default());
        assert!(!pure.is_text_comparison_complete());
        let identity =
            diff_bytes("same", "same", &new, &new, &limits, &DiffOptions::default()).unwrap();
        assert!(identity.is_comparison_complete());
        assert!(!identity.is_text_comparison_complete());
    }
}

#[test]
fn malformed_source_tail_is_reviewable_instead_of_silently_truncated() {
    let bytes = fixture::fixture("source_trailing_invalid", true);
    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    assert!(ir.pages[0].source_text_accounting_unavailable);
    assert_eq!(ir.pages[0].source_text_characters_missing, 0);
    let runs = text_layer_from_bytes(&bytes, 1).unwrap();
    assert!(runs.iter().all(|r| r.source_text_accounting_unavailable));
}

#[test]
fn shared_source_controls_keep_identical_observed_text_and_pixels_reviewable() {
    for name in fixture::CASES
        .iter()
        .filter(|name| name.starts_with("source_") && !name.starts_with("source_unicode_"))
    {
        let old = fixture::fixture(name, false);
        let new = fixture::fixture(name, true);
        let result = diff_bytes(
            "old",
            "new",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        let unavailable = *name == "source_trailing_invalid";
        assert!(!result.is_comparison_complete(), "{name}");
        assert!(!result.is_text_comparison_complete(), "{name}");
        assert_eq!(
            result.diagnostics.new_pages_with_missing_source_characters,
            usize::from(!unavailable),
            "{name}"
        );
        assert_eq!(
            result
                .diagnostics
                .new_pages_with_unavailable_source_accounting,
            usize::from(unavailable),
            "{name}"
        );
        assert_eq!(
            result.summary.blocks_changed
                + result.summary.blocks_added
                + result.summary.blocks_removed,
            0,
            "{name}"
        );
        assert_eq!(
            result.summary.pages_visual_changed,
            usize::from(*name == "source_duplicate_visible"),
            "{name}"
        );
    }
}

#[test]
fn unicode_expansion_cannot_mask_another_missing_source_glyph() {
    for (unit, rotation, nesting, mode) in [
        (1.0, 0, 0, "masked"),
        (1.0, 90, 2, "masked"),
        (64.0, 270, 0, "masked"),
        (1.0, 180, 0, "range"),
    ] {
        let old = fixture::source_unicode(unit, rotation, nesting, mode, false);
        let new = fixture::source_unicode(unit, rotation, nesting, mode, true);
        let limits = Limits::default();
        let ir = load_document_from_bytes(&new, &limits).unwrap();
        let page = &ir.pages[0];
        assert_eq!(page.text().replace(char::is_whitespace, ""), "CCC");
        assert_eq!(page.source_text_characters_missing, 0); // An expansion masks the encoded-glyph deficit.
        assert_eq!(page.sdk_text_objects_without_characters, 0);
        assert_eq!(page.source_unicode_scalars_unmatched, 1);
        assert!(!page.source_unicode_accounting_unavailable);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("new.pdf");
        let before = temp.path().join("old.pdf");
        std::fs::write(&path, &new).unwrap();
        std::fs::write(&before, &old).unwrap();
        assert_eq!(
            serde_json::to_value(&ir).unwrap(),
            serde_json::to_value(load_document(&path, &limits).unwrap()).unwrap()
        );
        for layer in [text_layer(&path, 1), text_layer_from_bytes(&new, 1)] {
            let runs = layer.unwrap();
            assert!(runs.iter().all(|r| r.source_unicode_scalars_unmatched == 1
                && !r.source_unicode_accounting_unavailable));
        }
        let result = diff_bytes("a", "b", &old, &new, &limits, &DiffOptions::default()).unwrap();
        assert!(!result.is_comparison_complete());
        assert!(!result.is_text_comparison_complete());
        assert!(!result.has_changes());
        assert_eq!(result.diagnostics.old_pages_with_source_unicode_mismatch, 0);
        assert_eq!(result.diagnostics.new_pages_with_source_unicode_mismatch, 1);
        let files = pdf_diff::diff_files(&before, &path, &limits, &DiffOptions::default()).unwrap();
        assert_eq!(
            serde_json::to_value(&result.diagnostics).unwrap(),
            serde_json::to_value(files.diagnostics).unwrap()
        );
        assert!(
            !pdf_diff::diff_documents("a", "b", &ir, &ir, &DiffOptions::default())
                .is_text_comparison_complete()
        );
        let identity = diff_bytes("a", "a", &new, &new, &limits, &DiffOptions::default()).unwrap();
        assert!(identity.is_comparison_complete());
        assert!(!identity.is_text_comparison_complete());
    }
}

#[test]
fn marked_actual_text_overrides_glyph_mapping_and_empty_replacement_is_recovered() {
    for mode in [
        "actual",
        "actual_named",
        "actual_empty",
        "actual_blank",
        "actual_large",
    ] {
        let old = fixture::source_unicode(1.0, 0, 2, mode, false);
        let new = fixture::source_unicode(1.0, 0, 2, mode, true);
        let result = diff_bytes(
            "a",
            "b",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        let ir = load_document_from_bytes(&new, &Limits::default()).unwrap();
        assert!(!ir.pages[0].source_unicode_accounting_unavailable);
        if matches!(mode, "actual_empty" | "actual_blank") {
            assert_eq!(ir.pages[0].text(), "");
            assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
            assert!(result.is_comparison_complete());
            assert!(result
                .pages
                .iter()
                .flat_map(|p| &p.blocks)
                .any(|b| b.old_text.as_deref() == Some("C") && b.new_text.is_none()));
            assert!(text_layer_from_bytes(&new, 1).unwrap().is_empty());
        } else {
            assert_eq!(
                ir.pages[0].text(),
                if mode == "actual_large" {
                    "D".repeat(3000)
                } else {
                    "D".into()
                }
            );
            assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
            assert!(result.is_comparison_complete());
            assert!(result.is_text_comparison_complete());
        }
        assert_eq!(result.summary.pages_visual_changed, 0);
        assert!(result.has_changes());
    }
}

#[test]
fn source_anchors_recover_zero_ink_text_and_invalid_unicode_stays_reviewable() {
    for (mode, empty) in [("actual_graphics", true), ("actual_invalid", false)] {
        let bytes = fixture::source_unicode(1.0, 0, 0, mode, true);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("source.pdf");
        std::fs::write(&path, &bytes).unwrap();
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        let page = &ir.pages[0];
        assert_eq!(page.source_text_characters_missing, 0);
        assert_eq!(page.sdk_text_objects_without_characters, 0);
        assert_eq!(page.source_unicode_accounting_unavailable, !empty);
        for layer in [text_layer(&path, 1), text_layer_from_bytes(&bytes, 1)] {
            if empty {
                assert_eq!(page.source_unicode_scalars_unmatched, 0);
                assert_eq!(page.text(), "D");
                assert!(!page.reading_order_requires_review);
                let runs = layer.unwrap();
                assert_eq!(runs.len(), 1);
                assert!(!runs[0].source_text_anchor && runs[0].source_text_region);
                // The public SDK reports a positive stroke rectangle even
                // for this zero-length path. It is a whole source region.
                assert!(runs[0].bounds.width() > 0.0);
                assert!(runs[0].bounds.height() > 0.0);
                assert_eq!(runs[0].source_regions.len(), 1);
                assert_eq!(runs[0].source_regions[0].bounds, runs[0].bounds);
            } else {
                let runs = layer.unwrap();
                assert!(!runs.is_empty());
                assert!(runs.iter().all(|r| r.source_unicode_accounting_unavailable));
            }
        }
    }
}

/// 抽出処理を通さず、ホストの代替フォントが SDK に文字を供給するか確認する。
fn sdk_has_character_records(bytes: &[u8]) -> bool {
    use pdfium_render::prelude::Pdfium;
    let pdfium =
        Pdfium::new(Pdfium::bind_to_library(pdf_diff::pdfium_library_path().unwrap()).unwrap());
    let document = pdfium.load_pdf_from_byte_slice(bytes, None).unwrap();
    let page = document.pages().get(0).unwrap();
    let text = page.text().unwrap();
    !text.chars().is_empty()
}

#[test]
fn original_adobe_names_precede_uni_u_prefixes_and_unknown_names_stay_uncertain() {
    for (mode, known) in [
        ("union", true),
        ("universal", true),
        ("uacute", true),
        ("ugrave", true),
        ("nonbmp", true),
        ("multicomponent", true),
        ("alias", false),
        ("surrogate", false),
        ("lowercase", false),
        ("mixed_unknown", false),
    ] {
        let bytes = fixture::glyph_named_text(mode, true);
        let observed = sdk_has_character_records(&bytes);
        let ir = pdf_diff::load_document_from_bytes(&bytes, &pdf_diff::Limits::default()).unwrap();
        assert_eq!(
            !ir.pages[0].source_unicode_accounting_unavailable, known,
            "{mode}"
        );
        let expected = match mode {
            "union" => Some("∪"),
            "universal" => Some("∀"),
            "uacute" => Some("ú"),
            "ugrave" => Some("ù"),
            "nonbmp" => Some("𐐌"),
            _ => None,
        };
        if let Some(expected) = expected.filter(|_| observed) {
            assert_eq!(ir.pages[0].text(), expected, "{mode}");
            assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0, "{mode}");
            assert_eq!(ir.pages[0].sdk_unicode_mapping_errors, 0, "{mode}");
        }
        if mode == "multicomponent" && observed {
            assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 6);
        }
        if !observed {
            let page = &ir.pages[0];
            assert_eq!(page.text(), "", "{mode}");
            assert_eq!(page.sdk_unicode_mapping_errors, 0, "{mode}");
            assert_eq!(page.sdk_text_objects_without_characters, 1, "{mode}");
            assert_eq!(page.source_text_characters_missing, 1, "{mode}");
            assert_eq!(
                page.source_unicode_scalars_unmatched,
                if mode == "multicomponent" {
                    5
                } else {
                    usize::from(known)
                },
                "{mode}"
            );
            let error = pdf_diff::text_layer_from_bytes(&bytes, 1).unwrap_err();
            assert!(
                error.to_string().contains("no character records"),
                "{mode}: {error}"
            );
        }
        if !known || !observed {
            let old = fixture::glyph_named_text(mode, false);
            let result = pdf_diff::diff_bytes(
                "a",
                "b",
                &old,
                &bytes,
                &pdf_diff::Limits::default(),
                &pdf_diff::DiffOptions::default(),
            )
            .unwrap();
            assert!(!result.is_comparison_complete(), "{mode}");
            assert_eq!(
                result.diagnostics.new_pages_with_unavailable_source_unicode,
                usize::from(!known),
                "{mode}"
            );
            if !observed {
                assert_eq!(
                    result.diagnostics.new_pages_with_unobserved_text_objects, 1,
                    "{mode}"
                );
                assert_eq!(
                    result.diagnostics.new_pages_with_missing_source_characters, 1,
                    "{mode}"
                );
            }
        }
    }
}
