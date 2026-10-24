#[path = "support/appearance_fixture.rs"]
mod fixture;
use pdf_diff::{
    diff_bytes, render_page_from_bytes, DiffOptions, Limits, VisualDiffOptions, VisualStatus,
};

#[test]
fn crop_changes_that_cross_furniture_bands_keep_the_same_table_page() {
    let old = fixture::fixture("crop_table_identity", false);
    let new = fixture::fixture("crop_table_identity", true);
    let a = pdf_diff::load_document_from_bytes(&old, &Limits::default()).unwrap();
    let b = pdf_diff::load_document_from_bytes(&new, &Limits::default()).unwrap();
    assert_eq!(a.pages[0].text(), b.pages[0].text());
    assert_ne!(a.pages[0].body_text(), b.pages[0].body_text());
    let result = diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(result.summary.pages_added, 0);
    assert_eq!(result.summary.pages_removed, 0);
    assert_eq!(result.summary.pages_geometry_changed, 1);
    assert_eq!(result.summary.blocks_changed, 0);
    assert_eq!(result.summary.pages_visual_changed, 1);
    // The source deliberately paints a row-level space-only object. Its
    // omission is separate from the correctly matched table/page geometry.
    assert!(!result.is_comparison_complete());
    assert_eq!(result.diagnostics.old_pages_with_unobserved_text_objects, 1);
    assert_eq!(result.diagnostics.new_pages_with_unobserved_text_objects, 1);
}

#[test]
fn coordinate_basis_changes_preserve_physical_text_and_painting() {
    for (case, changed) in [
        ("layout_units", 0),
        ("layout_units_edit", 1),
        ("construction_units", 0),
        ("construction_shared_units", 0),
        ("tagged_table_units", 0),
        ("tagged_table_edit", 1),
        ("tagged_table_spans_units", 0),
        ("tagged_table_spans_rtl", 0),
    ] {
        let old = fixture::fixture(case, false);
        let new = fixture::fixture(case, true);
        let diff = diff_bytes(
            "old",
            "new",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        let whitespace_object = matches!(case, "tagged_table_units" | "tagged_table_edit");
        assert_eq!(
            diff.is_comparison_complete(),
            !whitespace_object,
            "{case}: {:?}",
            diff.diagnostics
        );
        assert_eq!(
            diff.diagnostics.old_pages_with_unobserved_text_objects,
            usize::from(whitespace_object),
            "{case}"
        );
        assert_eq!(
            diff.diagnostics.new_pages_with_unobserved_text_objects,
            usize::from(whitespace_object),
            "{case}"
        );
        assert_eq!(diff.summary.pages_geometry_changed, 1);
        assert_eq!(diff.summary.blocks_added, 0);
        assert_eq!(diff.summary.blocks_removed, 0);
        assert_eq!(diff.summary.blocks_changed, changed);
        assert_eq!(diff.summary.blocks_moved, 0);
        assert_eq!(diff.summary.blocks_format_changed, 0);
        assert_eq!(diff.summary.pages_visual_changed, changed);
        if changed == 0 {
            assert_eq!(diff.summary.blocks_resized, 0);
        }
        let b = &diff.pages[0].blocks[0];
        assert_eq!(b.old_font_size, Some(12.0));
        assert_eq!(b.new_font_size, Some(12.0 / 64.0));
        assert!((b.old_bounds.unwrap().x0 - b.new_bounds.unwrap().x0 * 64.0).abs() < 0.001);
    }
}

#[test]
fn shared_radio_controls_match_physical_rectangles_across_units() {
    for (case, changes) in [("widget_units", 0), ("widget_units_edit", 2)] {
        let old = fixture::fixture(case, false);
        let new = fixture::fixture(case, true);
        let diff = diff_bytes(
            "old",
            "new",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(!diff.is_comparison_complete());
        assert_eq!(diff.summary.form_fields_changed, changes);
        assert_eq!(diff.comparison.as_ref().unwrap().widget_fields_examined, 4);
        assert_eq!(
            diff.summary.canonical_form_fields_changed,
            usize::from(changes > 0)
        );
        for f in &diff.pages[0].form_changes {
            assert!(f.old.is_some() && f.new.is_some());
            let (a, b) = (f.old.as_ref().unwrap(), f.new.as_ref().unwrap());
            assert_eq!(a.name, b.name);
            assert!((a.bounds.x0 - b.bounds.x0 * 72.0).abs() < 0.001);
        }
    }
}

#[test]
fn image_vector_and_text_appearance_changes_are_detected_with_unchanged_text() {
    for case in [
        "partial_scan",
        "image_only",
        "path",
        "text_color",
        "text_face",
        "line_hyphen",
    ] {
        let a = fixture::fixture(case, false);
        let b = fixture::fixture(case, true);
        let diff = diff_bytes(
            "old",
            "new",
            &a,
            &b,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(diff.has_changes(), "{case}");
        assert_eq!(diff.summary.pages_visual_changed, 1, "{case}");
        assert!(
            diff.is_comparison_complete(),
            "{case}: {:?}",
            diff.comparison
        );
        let visual = diff.pages[0].visual.as_ref().unwrap();
        assert_eq!(visual.status, VisualStatus::Changed);
        assert!(visual.changed_pixels.unwrap() > 0);
        if case != "text_face" && case != "line_hyphen" {
            assert_eq!(diff.summary.blocks_changed, 0, "{case}");
        }
        if case == "line_hyphen" {
            assert!(diff.pages[0]
                .blocks
                .iter()
                .any(|b| b.old_text.as_deref() == Some("well-")
                    && b.new_text.as_deref() == Some("well=")));
        }
        if case == "path" {
            let b = visual.new_bounds.unwrap();
            for (actual, expected) in [(b.x0, 70.0), (b.y0, 80.0), (b.x1, 110.0), (b.y1, 100.0)] {
                assert!((actual - expected).abs() <= 1.0, "{b:?}");
            }
        }
    }
}

#[test]
fn changing_lossless_image_encoding_does_not_create_an_appearance_difference() {
    let a = fixture::fixture("image_reencoded", false);
    let b = fixture::fixture("image_reencoded", true);
    assert_ne!(a, b);
    let diff = diff_bytes(
        "a",
        "b",
        &a,
        &b,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!diff.has_changes());
    assert!(diff.is_comparison_complete());
    assert_eq!(
        diff.pages[0].visual.as_ref().unwrap().changed_pixels,
        Some(0)
    );
}

#[test]
fn logical_widget_changes_are_detected_even_with_identical_appearances() {
    for case in [
        "form_value",
        "form_flags",
        "form_hidden",
        "form_unicode",
        "checkbox",
        "checkbox_export",
        "checkbox_unchecked_export",
        "list",
        "combo_custom",
        "combo_no_options",
        "choice_export_label",
    ] {
        let a = fixture::fixture(case, false);
        let b = fixture::fixture(case, true);
        assert_eq!(
            render_page_from_bytes(&a, 1, 400).unwrap().png,
            render_page_from_bytes(&b, 1, 400).unwrap().png,
            "{case}: fixture appearance should be stale/hidden"
        );
        let diff = diff_bytes(
            "a",
            "b",
            &a,
            &b,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(diff.has_changes(), "{case}: {diff:?}");
        assert_eq!(diff.summary.form_fields_changed, 1, "{case}");
        assert_eq!(diff.summary.pages_visual_changed, 0, "{case}");
        assert!(
            !diff.is_comparison_complete(),
            "Unplaced canonical fields still require review"
        );
        let change = &diff.pages[0].form_changes[0];
        if case == "checkbox_export" {
            assert_eq!(change.old.as_ref().unwrap().checked, Some(true));
            assert_eq!(change.new.as_ref().unwrap().checked, Some(true));
        }
        if case == "checkbox_unchecked_export" {
            assert_eq!(change.old.as_ref().unwrap().checked, Some(false));
            assert_eq!(change.new.as_ref().unwrap().checked, Some(false));
            assert_eq!(
                change.old.as_ref().unwrap().value,
                change.new.as_ref().unwrap().value
            );
            assert_eq!(
                change.old.as_ref().unwrap().export_value.as_deref(),
                Some("Yes")
            );
            assert_eq!(
                change.new.as_ref().unwrap().export_value.as_deref(),
                Some("Yep")
            );
        }
        if case == "form_flags" {
            assert_eq!(
                change.old.as_ref().unwrap().value,
                change.new.as_ref().unwrap().value
            );
            assert_eq!(change.old.as_ref().unwrap().flags, 0);
            assert_eq!(change.new.as_ref().unwrap().flags, 3);
        }
        if case == "form_unicode" {
            assert_eq!(
                change.new.as_ref().unwrap().value.as_deref(),
                Some("山田花子")
            );
        }
        if case == "choice_export_label" {
            assert_eq!(change.old.as_ref().unwrap().value.as_deref(), Some("foo"));
            assert_eq!(change.new.as_ref().unwrap().value.as_deref(), Some("bar"));
            assert_eq!(
                change.old.as_ref().unwrap().display_value.as_deref(),
                Some("Foo")
            );
            assert_eq!(
                change.new.as_ref().unwrap().display_value.as_deref(),
                Some("Bar")
            );
        }
        if case == "combo_custom" || case == "combo_no_options" {
            assert_eq!(
                change.new.as_ref().unwrap().value.as_deref(),
                Some("Typed new")
            );
        }
        if case == "list" {
            assert_eq!(
                change.new.as_ref().unwrap().selected_options,
                vec![(0, Some("A".into())), (2, Some("C".into()))]
            );
        }
    }
}

#[test]
fn annotation_and_radio_control_order_do_not_change_field_identity() {
    let a = fixture::fixture("radio_reordered", false);
    let b = fixture::fixture("radio_reordered", true);
    let diff = diff_bytes(
        "a",
        "b",
        &a,
        &b,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(diff.summary.form_fields_changed, 0, "{diff:?}");
    assert!(!diff.has_changes(), "{diff:?}");
    assert_eq!(diff.comparison.as_ref().unwrap().widget_fields_examined, 4);
}

#[test]
fn identical_source_proof_and_visual_budget_limits_are_explicit() {
    let a = fixture::fixture("budget", false);
    let b = fixture::fixture("budget", true);
    let options = DiffOptions {
        visual: Some(VisualDiffOptions {
            max_total_pixels: 720_000,
            ..VisualDiffOptions::default()
        }),
        ..DiffOptions::default()
    };
    let diff = diff_bytes("a", "b", &a, &b, &Limits::default(), &options).unwrap();
    assert!(!diff.is_comparison_complete());
    let coverage = diff.comparison.as_ref().unwrap();
    assert_eq!(coverage.rendered_page_pairs, 1);
    assert_eq!(coverage.skipped_page_pairs, 1);
    assert_eq!(
        diff.pages[1].visual.as_ref().unwrap().status,
        VisualStatus::Skipped
    );
    let same = diff_bytes("a", "a", &a, &a, &Limits::default(), &options).unwrap();
    assert!(same.is_comparison_complete());
    assert!(same.comparison.unwrap().identical_source);
    let no_visual = diff_bytes(
        "a",
        "b",
        &a,
        &b,
        &Limits::default(),
        &DiffOptions {
            visual: None,
            ..DiffOptions::default()
        },
    )
    .unwrap();
    assert!(!no_visual.is_comparison_complete());
}

#[test]
fn visual_configuration_is_validated_and_channel_tolerance_is_observable() {
    let a = fixture::fixture("partial_scan", false);
    let b = fixture::fixture("partial_scan", true);
    let diff = diff_bytes(
        "a",
        "b",
        &a,
        &b,
        &Limits::default(),
        &DiffOptions {
            visual: Some(VisualDiffOptions {
                channel_tolerance: 255,
                ..VisualDiffOptions::default()
            }),
            ..DiffOptions::default()
        },
    )
    .unwrap();
    assert!(!diff.has_changes());
    for visual in [
        VisualDiffOptions {
            dpi: 0,
            ..VisualDiffOptions::default()
        },
        VisualDiffOptions {
            dpi: 601,
            ..VisualDiffOptions::default()
        },
        VisualDiffOptions {
            max_total_pixels: 0,
            ..VisualDiffOptions::default()
        },
    ] {
        assert!(DiffOptions {
            visual: Some(visual),
            ..DiffOptions::default()
        }
        .validate()
        .is_err());
    }
    let one = diff_bytes(
        "a",
        "b",
        &a,
        &b,
        &Limits::default(),
        &DiffOptions {
            visual: Some(VisualDiffOptions {
                dpi: 72,
                ..VisualDiffOptions::default()
            }),
            ..DiffOptions::default()
        },
    )
    .unwrap();
    assert_eq!(one.comparison.unwrap().pixel_budget_used, 180_000);
}

#[test]
fn oversized_visual_pages_remain_explicitly_unverified() {
    let a = fixture::fixture("oversized", false);
    let b = fixture::fixture("oversized", true);
    let diff = diff_bytes(
        "a",
        "b",
        &a,
        &b,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!diff.is_comparison_complete());
    let visual = diff.pages[0].visual.as_ref().unwrap();
    assert_eq!(visual.status, VisualStatus::Skipped);
    assert!(visual.reason.as_ref().unwrap().contains("rendering limit"));
}

#[test]
fn huge_image_sources_are_rejected_before_decoding_including_nested_forms() {
    for case in ["oversized_image", "nested_oversized_image"] {
        let a = fixture::fixture(case, false);
        let b = fixture::fixture(case, true);
        let diff = diff_bytes(
            "a",
            "b",
            &a,
            &b,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(!diff.is_comparison_complete());
        let visual = diff.pages[0].visual.as_ref().unwrap();
        assert_eq!(visual.status, VisualStatus::Skipped);
        assert!(
            visual
                .reason
                .as_ref()
                .unwrap()
                .contains("source image pixel limit"),
            "{visual:?}"
        );
        let error = render_page_from_bytes(&a, 1, 400).err().unwrap();
        assert!(error.to_string().contains("source image pixel limit"));
    }
    let visual = VisualDiffOptions {
        max_source_image_pixels: 0,
        ..VisualDiffOptions::default()
    };
    assert!(visual.validate().is_err());
    let a = fixture::fixture("partial_scan", false);
    let b = fixture::fixture("partial_scan", true);
    let limited = diff_bytes(
        "a",
        "b",
        &a,
        &b,
        &Limits::default(),
        &DiffOptions {
            visual: Some(VisualDiffOptions {
                max_source_image_pixels: 3,
                ..VisualDiffOptions::default()
            }),
            ..DiffOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        limited.pages[0].visual.as_ref().unwrap().status,
        VisualStatus::Skipped
    );
    assert_eq!(limited.comparison.as_ref().unwrap().pixel_budget_used, 0);
}

#[test]
fn file_and_byte_apis_and_cli_report_the_same_new_change_layers() {
    for (case, exit) in [
        ("partial_scan", 1),
        ("image_reencoded", 0),
        ("form_value", 3),
        ("oversized", 3),
    ] {
        let a = fixture::fixture(case, false);
        let b = fixture::fixture(case, true);
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.pdf");
        let new = dir.path().join("new.pdf");
        std::fs::write(&old, &a).unwrap();
        std::fs::write(&new, &b).unwrap();
        let native =
            pdf_diff::diff_files(&old, &new, &Limits::default(), &DiffOptions::default()).unwrap();
        let memory = diff_bytes(
            "a",
            "b",
            &a,
            &b,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&native.summary).unwrap(),
            serde_json::to_value(&memory.summary).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&native.comparison).unwrap(),
            serde_json::to_value(&memory.comparison).unwrap()
        );
        let cli = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
            .arg(&old)
            .arg(&new)
            .output()
            .unwrap();
        assert_eq!(
            cli.status.code(),
            Some(exit),
            "{case}: {}",
            String::from_utf8_lossy(&cli.stderr)
        );
    }
}

#[test]
fn text_widget_names_and_values_are_bounded_in_byte_file_and_cli_apis() {
    for case in ["oversized_widget_name", "oversized_widget_value"] {
        let a = fixture::fixture(case, false);
        let b = fixture::fixture(case, true);
        let error = diff_bytes(
            "a",
            "b",
            &a,
            &b,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("2000000 UTF-16"),
            "{case}: {error:#}"
        );
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.pdf");
        let new = dir.path().join("new.pdf");
        std::fs::write(&old, &a).unwrap();
        std::fs::write(&new, &b).unwrap();
        let error = pdf_diff::diff_files(&old, &new, &Limits::default(), &DiffOptions::default())
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("2000000 UTF-16"),
            "{case}: {error:#}"
        );
        let cli = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
            .arg(&old)
            .arg(&new)
            .output()
            .unwrap();
        assert_eq!(cli.status.code(), Some(2), "{case}");
        assert!(
            cli.stdout.is_empty(),
            "An over-limit read must not emit a partial diff"
        );
        // Owned byte equality is a stronger proof and does not require string allocation.
        let same = diff_bytes(
            "b",
            "b",
            &b,
            &b,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(same.is_comparison_complete());
    }
}

#[test]
fn physical_page_unit_fixtures_match_browser_contract() {
    for name in ["user_unit_scale", "user_unit_invalid", "user_unit_limit"] {
        let old = fixture::fixture(name, false);
        let new = fixture::fixture(name, true);
        let diff = diff_bytes(
            "old",
            "new",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(diff_bytes(
            "old",
            "old",
            &old,
            &old,
            &Limits::default(),
            &DiffOptions::default()
        )
        .unwrap()
        .is_comparison_complete());
        if name == "user_unit_scale" {
            assert_eq!(diff.summary.pages_geometry_changed, 1);
            assert_eq!(diff.summary.pages_visual_changed, 1);
            assert!(diff.is_comparison_complete());
            let visual = diff.pages[0].visual.as_ref().unwrap();
            assert_eq!(visual.old_viewport.unwrap().width, 600);
            assert_eq!(visual.new_viewport.unwrap().width, 1800);
        } else {
            assert!(!diff.is_comparison_complete());
            let coverage = diff.comparison.unwrap();
            assert_eq!(coverage.skipped_page_pairs, 1);
            assert_eq!(coverage.pixel_budget_used, 0);
        }
    }
}

#[test]
fn empty_acroform_shells_have_complete_byte_file_and_cli_comparisons() {
    for (case, changed) in [("empty_form_shell", false), ("empty_form_shell_edit", true)] {
        let old = fixture::fixture(case, false);
        let new = fixture::fixture(case, true);
        let diff = diff_bytes(
            "old",
            "new",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            diff.is_comparison_complete(),
            "{case}: {:?}",
            diff.comparison
        );
        assert!(diff.is_text_comparison_complete());
        assert_eq!(diff.has_changes(), changed);
        assert_eq!(diff.summary.blocks_changed, usize::from(changed));
        assert_eq!(diff.summary.pages_visual_changed, usize::from(changed));
        let coverage = diff.comparison.as_ref().unwrap();
        assert!(!coverage.identical_source);
        assert_eq!(coverage.verified_empty_acroform_shells, 2);
        assert_eq!(coverage.interactive_pages_requiring_review, 0);
        assert_eq!(coverage.widget_fields_examined, 0);
        assert_eq!(coverage.canonical_fields_examined, 0);
        assert!(coverage.canonical_form_review_reasons.is_empty());
        let a = tempfile::NamedTempFile::new().unwrap();
        let b = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(a.path(), &old).unwrap();
        std::fs::write(b.path(), &new).unwrap();
        let files = pdf_diff::diff_files(
            a.path(),
            b.path(),
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&files.summary).unwrap(),
            serde_json::to_value(&diff.summary).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&files.comparison).unwrap(),
            serde_json::to_value(coverage).unwrap()
        );
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
            .arg(a.path())
            .arg(b.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(i32::from(changed)));
        let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output["comparison"]["verified_empty_acroform_shells"], 2);
        assert_eq!(
            output["comparison"]["interactive_pages_requiring_review"],
            0
        );
    }
    for mode in [
        "empty",
        "defaults",
        "indirect-defaults",
        "empty-annots",
        "structure-title",
        "linearization",
        "hint-stream",
        "xfa-null",
    ] {
        for indirect in [false, true] {
            let old = fixture::empty_form_shell(mode, false, indirect);
            let new = fixture::empty_form_shell("no-form", false, false);
            let diff = diff_bytes(
                "old",
                "new",
                &old,
                &new,
                &Limits::default(),
                &DiffOptions::default(),
            )
            .unwrap();
            assert!(
                diff.is_comparison_complete(),
                "{mode}/{indirect}: {:?}",
                diff.comparison
            );
            assert!(!diff.has_changes());
            assert_eq!(
                diff.comparison
                    .as_ref()
                    .unwrap()
                    .verified_empty_acroform_shells,
                1
            );
        }
    }
    let mut legacy = serde_json::to_value(pdf_diff::ComparisonCoverage::default()).unwrap();
    assert!(legacy.get("verified_empty_acroform_shells").is_none());
    legacy
        .as_object_mut()
        .unwrap()
        .remove("verified_empty_acroform_shells");
    let decoded: pdf_diff::ComparisonCoverage = serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.verified_empty_acroform_shells, 0);
}

#[test]
fn empty_fields_do_not_hide_orphan_widgets_fields_actions_or_malformed_structures() {
    let good = fixture::empty_form_shell("defaults", false, false);
    for mode in [
        "missing",
        "null",
        "number",
        "dangling",
        "cycle",
        "field-tree",
        "unplaced",
        "bare-field",
        "nested-field",
        "stream-field",
        "orphan-widget",
        "annotation",
        "malformed-annots",
        "signature",
        "xfa",
        "action",
        "form-action",
        "names",
        "permissions",
        "need-appearances",
        "unknown",
        "calculation-order",
        "signature-flags",
        "structure-field",
    ] {
        let bad = fixture::empty_form_shell(mode, false, true);
        for (old, new) in [(&good, &bad), (&bad, &good)] {
            let diff = diff_bytes(
                "old",
                "new",
                old,
                new,
                &Limits::default(),
                &DiffOptions::default(),
            )
            .unwrap();
            let coverage = diff.comparison.as_ref().unwrap();
            assert!(!diff.is_comparison_complete(), "{mode}: {coverage:?}");
            assert_eq!(
                coverage.verified_empty_acroform_shells, 1,
                "{mode}: {coverage:?}"
            );
            assert!(
                coverage.interactive_pages_requiring_review > 0,
                "{mode}: {coverage:?}"
            );
        }
    }
}
