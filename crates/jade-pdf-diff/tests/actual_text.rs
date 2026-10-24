#[allow(dead_code)]
#[path = "support/appearance_fixture.rs"]
mod fixture;
use pdf_diff::{
    diff_bytes, diff_files, load_document, load_document_from_bytes, text_layer,
    text_layer_from_bytes, DiffOptions, Limits,
};

#[test]
fn structure_actual_text_recovers_logical_edits_for_text_forms_and_vectors() {
    for mode in [
        "page",
        "form",
        "parent_form",
        "vector",
        "multiple",
        "descendant",
        "unicode",
    ] {
        let old = fixture::structure_actual(1.0, 0, mode, false);
        let new = fixture::structure_actual(1.0, 0, mode, true);
        let limits = Limits::default();
        let options = DiffOptions::default();
        let temp = tempfile::tempdir().unwrap();
        let old_path = temp.path().join("old.pdf");
        let path = temp.path().join("new.pdf");
        std::fs::write(&old_path, &old).unwrap();
        std::fs::write(&path, &new).unwrap();
        let ir = load_document_from_bytes(&new, &limits).unwrap();
        let expected = if mode == "unicode" {
            "日本語🦊"
        } else {
            "B"
        };
        assert_eq!(ir.pages[0].text(), expected, "{mode}");
        assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0, "{mode}");
        assert_eq!(ir.pages[0].source_text_characters_missing, 0, "{mode}");
        assert!(!ir.pages[0].source_unicode_accounting_unavailable, "{mode}");
        assert!(!ir.pages[0].source_text_accounting_unavailable, "{mode}");
        assert_eq!(ir.pages[0].sdk_text_objects_without_characters, 0, "{mode}");
        assert_eq!(
            serde_json::to_value(&ir).unwrap(),
            serde_json::to_value(load_document(&path, &limits).unwrap()).unwrap()
        );
        for layer in [text_layer(&path, 1), text_layer_from_bytes(&new, 1)] {
            let runs = layer.unwrap();
            assert_eq!(runs.len(), 1, "{mode}");
            let run = &runs[0];
            assert_eq!(run.text, expected);
            assert!(run.source_text_region);
            assert_eq!(run.chars.len(), expected.chars().count());
            assert!(run.bounds.x0 < run.bounds.x1 && run.bounds.y0 < run.bounds.y1);
            assert!(run
                .chars
                .iter()
                .all(|c| c.x0 == run.bounds.x0 && c.x1 == run.bounds.x1));
        }
        let result = diff_bytes("a", "b", &old, &new, &limits, &options).unwrap();
        assert_eq!(result.summary.blocks_changed, 1, "{mode}");
        assert_eq!(result.summary.pages_visual_changed, 0, "{mode}");
        assert!(
            result.is_comparison_complete(),
            "{mode}: {:?}",
            result.diagnostics
        );
        assert!(result.is_text_comparison_complete(), "{mode}");
        let files = diff_files(&old_path, &path, &limits, &options).unwrap();
        assert_eq!(
            serde_json::to_value(files.summary).unwrap(),
            serde_json::to_value(result.summary).unwrap()
        );
        assert_eq!(
            serde_json::to_value(files.diagnostics).unwrap(),
            serde_json::to_value(result.diagnostics).unwrap()
        );
        let decoded = serde_json::from_value(serde_json::to_value(&ir).unwrap()).unwrap();
        let pure = pdf_diff::diff_documents("a", "b", &ir, &decoded, &options);
        assert!(pure.is_text_comparison_complete());
        assert!(!pure.has_changes());
        assert_eq!(new, std::fs::read(&path).unwrap());
    }
}

#[test]
fn source_regions_keep_unrotated_geometry_and_respect_user_units() {
    for unit in [1.0, 64.0] {
        for mode in [
            "page",
            "form",
            "parent_form",
            "vector",
            "multiple",
            "unicode",
        ] {
            let reference = fixture::structure_actual(unit, 0, mode, true);
            let layer = text_layer_from_bytes(&reference, 1).unwrap();
            for rotation in [0, 90, 180, 270] {
                let bytes = fixture::structure_actual(unit, rotation, mode, true);
                let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(
                    ir.pages[0].text(),
                    if mode == "unicode" {
                        "日本語🦊"
                    } else {
                        "B"
                    }
                );
                assert_eq!(
                    serde_json::to_value(&layer).unwrap(),
                    serde_json::to_value(text_layer_from_bytes(&bytes, 1).unwrap()).unwrap(),
                    "{unit}/{rotation}/{mode}"
                );
                let run = &layer[0];
                assert!(
                    run.bounds.x0 * unit >= 29.9 && run.bounds.x1 * unit <= 61.0,
                    "{unit}/{mode}: {:?}",
                    run.bounds
                );
                assert!(run.bounds.y0 * unit >= 129.9 && run.bounds.y1 * unit <= 160.0);
            }
        }
    }
}

#[test]
fn empty_and_whitespace_replacements_suppress_underlying_characters() {
    for mode in ["empty", "space"] {
        let old = fixture::structure_actual(1.0, 0, mode, false);
        let new = fixture::structure_actual(1.0, 0, mode, true);
        let limits = Limits::default();
        let ir = load_document_from_bytes(&new, &limits).unwrap();
        assert!(ir.pages[0].blocks.is_empty());
        assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
        assert!(!ir.pages[0].source_unicode_accounting_unavailable);
        assert!(text_layer_from_bytes(&new, 1).unwrap().is_empty());
        let result = diff_bytes(
            "a",
            "b",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(result.has_changes(), "{mode}");
        assert!(
            result
                .pages
                .iter()
                .flat_map(|p| &p.blocks)
                .any(|b| b.old_text.as_deref() == Some("A") && b.new_text.is_none()),
            "{mode}"
        );
        assert_eq!(result.summary.pages_visual_changed, 0);
        assert!(result.is_comparison_complete());
    }
}

#[test]
fn ambiguous_source_ownership_keeps_raw_text_and_requires_review() {
    for mode in [
        "collision",
        "reused_form",
        "bad_parent",
        "bad_backpointer",
        "missing_parent_tree",
        "crosspage",
        "nested_actual",
    ] {
        let old = fixture::structure_actual(1.0, 0, mode, false);
        let new = fixture::structure_actual(1.0, 0, mode, true);
        let ir = load_document_from_bytes(&new, &Limits::default()).unwrap();
        assert!(
            ir.pages
                .iter()
                .all(|p| p.source_unicode_accounting_unavailable),
            "{mode}"
        );
        assert!(
            ir.pages
                .iter()
                .all(|p| p.source_text_accounting_unavailable),
            "{mode}"
        );
        assert!(ir.pages.iter().all(|p| !p.text().contains('B')), "{mode}");
        let layer = text_layer_from_bytes(&new, 1).unwrap();
        assert!(
            layer
                .iter()
                .all(|r| r.source_unicode_accounting_unavailable && !r.source_text_region),
            "{mode}"
        );
        let result = diff_bytes(
            "a",
            "b",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(!result.is_comparison_complete(), "{mode}");
        assert!(!result.is_text_comparison_complete(), "{mode}");
        assert_eq!(result.summary.blocks_changed, 0, "{mode}");
        let same = diff_bytes(
            "a",
            "a",
            &new,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(same.is_comparison_complete());
        assert!(!same.is_text_comparison_complete());
    }
}

#[test]
fn marked_scopes_count_each_source_declaration_once_across_objects_and_forms() {
    for mode in [
        "page",
        "mcid",
        "nested_mcid",
        "form",
        "outer_form",
        "reused_form",
        "zero",
        "zero_single",
        "zero_rotated",
    ] {
        let new = fixture::marked_actual_scopes(1., 0, mode, true);
        let ir = load_document_from_bytes(&new, &Limits::default()).unwrap();
        let expected = if mode == "reused_form" { "BBBB" } else { "BB" };
        assert_eq!(
            ir.pages[0].text().replace(char::is_whitespace, ""),
            expected,
            "{mode}"
        );
        assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0, "{mode}");
        assert!(
            !ir.pages[0].source_unicode_accounting_unavailable,
            "{mode}: {:?}",
            ir.pages[0]
        );
        assert!(!ir.pages[0].source_text_accounting_unavailable, "{mode}");
        assert_eq!(ir.pages[0].sdk_text_objects_without_characters, 0, "{mode}");
        let runs = text_layer_from_bytes(&new, 1).unwrap();
        assert!(runs.iter().all(|r| r.source_text_region), "{mode}");
        assert_eq!(
            runs.iter().any(|r| r.source_text_anchor),
            mode.starts_with("zero"),
            "{mode}"
        );
        let old = fixture::marked_actual_scopes(1., 0, mode, false);
        let result = diff_bytes(
            "old",
            "new",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(result.has_changes(), "{mode}");
        assert_eq!(
            result.summary.pages_added + result.summary.pages_removed,
            0,
            "{mode}"
        );
        assert!(
            result.summary.blocks_changed
                + result.summary.blocks_added
                + result.summary.blocks_removed
                > 0,
            "{mode}"
        );
        assert_eq!(result.summary.pages_visual_changed, 0, "{mode}");
        assert!(
            !result.is_comparison_complete(),
            "Multiple independent source scopes retain reading-order review: {mode}"
        );
        assert!(result.diagnostics.has_uncertain_reading_order(), "{mode}");
    }
}

#[test]
fn marked_scope_geometry_uses_form_ancestor_transforms_and_original_anchors() {
    for mode in ["page", "form", "outer_form", "zero_single"] {
        for unit in [1., 64.] {
            let reference = fixture::marked_actual_scopes(unit, 0, mode, true);
            let layer = text_layer_from_bytes(&reference, 1).unwrap();
            for rotation in [0, 90, 180, 270] {
                let bytes = fixture::marked_actual_scopes(unit, rotation, mode, true);
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("scope.pdf");
                std::fs::write(&path, &bytes).unwrap();
                let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(ir.pages[0].text().replace(char::is_whitespace, ""), "BB");
                for actual in [
                    text_layer_from_bytes(&bytes, 1).unwrap(),
                    text_layer(&path, 1).unwrap(),
                ] {
                    assert_eq!(
                        serde_json::to_value(actual).unwrap(),
                        serde_json::to_value(&layer).unwrap(),
                        "{mode}/{unit}/{rotation}"
                    );
                }
            }
        }
    }
    let base =
        text_layer_from_bytes(&fixture::marked_actual_scopes(1., 0, "page", true), 1).unwrap();
    let form =
        text_layer_from_bytes(&fixture::marked_actual_scopes(1., 0, "form", true), 1).unwrap();
    assert_eq!(base.len(), 2);
    assert_eq!(form.len(), 2);
    for (a, b) in base.iter().zip(form.iter()) {
        assert!(
            (b.bounds.x0 - (2. * a.bounds.x0 + 40.)).abs() < 0.001,
            "{:?}/{:?}",
            a.bounds,
            b.bounds
        );
        assert!((b.bounds.x1 - (2. * a.bounds.x1 + 40.)).abs() < 0.001);
        assert!((b.bounds.y0 - (a.bounds.y0 + 25.)).abs() < 0.001);
        assert!((b.bounds.y1 - (a.bounds.y1 + 25.)).abs() < 0.001);
        assert_eq!(a.size, b.size);
    }
    let anchored = text_layer_from_bytes(
        &fixture::marked_actual_scopes(1., 0, "zero_single", true),
        1,
    )
    .unwrap();
    assert_eq!(anchored.len(), 2);
    for (run, x) in anchored.iter().zip([30., 110.]) {
        assert!(run.source_text_anchor && run.source_text_region);
        assert_eq!(run.bounds.x0, x);
        assert_eq!(run.bounds.x1, x);
        assert_eq!(run.bounds.y0, 150.);
        assert_eq!(run.bounds.y1, 150.);
        assert_eq!(run.chars.len(), 1);
        assert_eq!(run.chars[0].x0, x);
        assert_eq!(run.chars[0].x1, x);
    }
}

#[test]
fn missing_mark_objects_and_conflicting_nested_actual_text_are_reviewed() {
    for mode in ["no_object", "nested_actual"] {
        let bytes = fixture::marked_actual_scopes(1., 0, mode, true);
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        assert!(ir.pages[0].source_unicode_accounting_unavailable, "{mode}");
        assert!(ir.pages[0].source_text_accounting_unavailable, "{mode}");
        let layer = text_layer_from_bytes(&bytes, 1);
        if mode == "no_object" {
            assert!(layer
                .unwrap_err()
                .to_string()
                .contains("Source text accounting is incomplete"));
        } else {
            assert_eq!(ir.pages[0].text(), "B X\nB X");
            assert!(layer.unwrap().iter().all(|r| !r.source_text_region));
        }
    }
}

#[test]
fn source_ranges_preserve_anchor_ownership_after_merging_and_serialization() {
    let bytes = fixture::marked_actual_scopes(1., 0, "mixed_anchor", true);
    let runs = text_layer_from_bytes(&bytes, 1).unwrap();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run.text, "B B");
    assert!(run.source_text_anchor && run.source_text_region);
    assert!(run.bounds.height() > 0.);
    assert_eq!(run.source_regions.len(), 2);
    assert_eq!(
        (run.source_regions[0].start, run.source_regions[0].end),
        (0, 1)
    );
    assert_eq!(
        (run.source_regions[1].start, run.source_regions[1].end),
        (2, 3)
    );
    assert!(run.source_regions[0].anchor);
    assert_eq!(
        run.source_regions[0].bounds,
        pdf_diff::Bounds {
            x0: 30.,
            x1: 30.,
            y0: 150.,
            y1: 150.
        }
    );
    assert!(!run.source_regions[1].anchor);
    assert!(run.source_regions[1].bounds.height() > 0.);
    let encoded = serde_json::to_value(run).unwrap();
    let decoded: pdf_diff::TextRun = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
    let source = fixture::structure_actual(1., 0, "unicode", true);
    let runs = text_layer_from_bytes(&source, 1).unwrap();
    assert_eq!(runs[0].text.chars().count(), 4);
    assert_eq!(runs[0].source_regions[0].end, 4);
    let legacy:pdf_diff::TextRun=serde_json::from_value(serde_json::json!({"text":"A","size":12.,"sdk_unicode_mapping_errors":0,"reading_order_requires_review":false,"bounds":{"x0":1.,"y0":2.,"x1":3.,"y1":4.},"chars":[{"x0":1.,"x1":3.}]})).unwrap();
    assert!(
        !legacy.source_text_anchor
            && !legacy.source_text_region
            && legacy.source_regions.is_empty()
    );
}

#[test]
fn declared_source_space_separates_words_even_without_a_geometric_gap() {
    let bytes = fixture::marked_space_between_touching_words(false);
    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text(), "A B");
    let runs = text_layer_from_bytes(&bytes, 1).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "A B");
    let source = &runs[0].source_regions;
    assert_eq!(source.len(), 1);
    assert_eq!((source[0].start, source[0].end), (1, 2));
    assert_eq!(&runs[0].text[source[0].start..source[0].end], " ");
}
