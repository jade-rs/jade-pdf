use pdf_diff::{
    diff_documents, try_diff_documents, BlockIr, Bounds, DiffOptions, DocumentIr, Limits, PageIr,
};

fn block(index: usize, text: &str, x: f64, y: f64) -> BlockIr {
    BlockIr {
        index,
        text: text.into(),
        bounds: Bounds {
            x0: x,
            y0: y,
            x1: x + 100.0,
            y1: y + 15.0,
        },
        size: Some(12.0),
    }
}

fn document() -> DocumentIr {
    DocumentIr {
        pages: vec![PageIr {
            index: 1,
            width: 600.0,
            height: 800.0,
            rotation: 0,
            view_box: None,
            user_unit: Some(1.0),
            sdk_unicode_mapping_errors: 0,
            sdk_text_objects_without_characters: 0,
            source_text_characters_missing: 0,
            source_text_accounting_unavailable: false,
            source_unicode_scalars_unmatched: 0,
            source_unicode_accounting_unavailable: false,
            reading_order_requires_review: false,
            blocks: vec![
                block(
                    1,
                    "This anchor paragraph identifies the unchanged page.",
                    100.0,
                    600.0,
                ),
                block(2, "Payment is due on day thirty", 100.0, 400.0),
            ],
        }],
    }
}

fn rebase(mut doc: DocumentIr, unit: f64) -> DocumentIr {
    for page in &mut doc.pages {
        page.user_unit = Some(unit);
        page.width /= unit;
        page.height /= unit;
        for block in &mut page.blocks {
            block.bounds.x0 /= unit;
            block.bounds.y0 /= unit;
            block.bounds.x1 /= unit;
            block.bounds.y1 /= unit;
            block.size = block.size.map(|size| size / unit);
        }
    }
    doc
}

#[test]
fn coordinate_rebasing_preserves_physical_text_layout_and_raw_output() {
    let old = document();
    let new = rebase(old.clone(), 64.0);
    let diff = try_diff_documents(
        "old",
        "new",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    // A coordinate-basis change remains page metadata, not a moved/resized font.
    assert_eq!(diff.summary.pages_geometry_changed, 1);
    assert_eq!(diff.summary.blocks_moved, 0);
    assert_eq!(diff.summary.blocks_resized, 0);
    assert_eq!(diff.summary.blocks_format_changed, 0);
    assert_eq!(
        diff.pages[0].blocks[0].old_bounds,
        Some(old.pages[0].blocks[0].bounds)
    );
    assert_eq!(
        diff.pages[0].blocks[0].new_bounds,
        Some(new.pages[0].blocks[0].bounds)
    );
    assert_eq!(diff.pages[0].blocks[0].old_font_size, Some(12.0));
    assert_eq!(diff.pages[0].blocks[0].new_font_size, Some(12.0 / 64.0));
    for (tolerance, moved) in [(1.0, 0), (0.01, 1)] {
        let mut shifted = new.clone();
        shifted.pages[0].blocks[1].bounds.x0 += 0.5 / 64.0;
        shifted.pages[0].blocks[1].bounds.x1 += 0.5 / 64.0;
        let options = DiffOptions {
            position_tolerance: tolerance,
            ..DiffOptions::default()
        };
        let diff = diff_documents("old", "shifted", &old, &shifted, &options);
        assert_eq!(diff.summary.blocks_moved, moved);
    }
    for (tolerance, formatted) in [(0.25, 0), (0.01, 1)] {
        let mut resized_font = new.clone();
        resized_font.pages[0].blocks[1].size = Some(12.1 / 64.0);
        let options = DiffOptions {
            font_size_tolerance: tolerance,
            ..DiffOptions::default()
        };
        let diff = diff_documents("old", "font", &old, &resized_font, &options);
        assert_eq!(diff.summary.blocks_format_changed, formatted);
    }
}

#[test]
fn changed_number_matches_the_same_physical_page_across_units() {
    let mut old = document();
    old.pages[0].blocks.truncate(1);
    old.pages[0].blocks[0].text = "111111".into();
    let mut new = rebase(old.clone(), 64.0);
    new.pages[0].blocks[0].text = "222222".into();
    let diff = diff_documents("old", "new", &old, &new, &DiffOptions::default());
    assert_eq!(diff.summary.pages_added, 0);
    assert_eq!(diff.summary.pages_removed, 0);
    assert_eq!(diff.summary.blocks_changed, 1);
    assert_eq!(diff.summary.blocks_added, 0);
    assert_eq!(diff.summary.blocks_removed, 0);
}

#[test]
fn checked_ir_rejects_physical_coordinate_overflow() {
    let old = document();
    for field in 0..4 {
        let mut new = rebase(old.clone(), 64.0);
        match field {
            0 => new.pages[0].width = 1e308,
            1 => {
                new.pages[0].blocks[0].bounds.x0 = 1e308;
                new.pages[0].blocks[0].bounds.x1 = 1e308;
            }
            2 => new.pages[0].blocks[0].size = Some(1e308),
            _ => {
                new.pages[0].user_unit = Some(1e-200);
                new.pages[0].blocks[0].size = Some(1e-200);
            }
        }
        assert!(try_diff_documents(
            "old",
            "new",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default()
        )
        .is_err());
    }
}

#[test]
fn text_change_score_recovers_a_relocated_edit_below_the_initial_match_threshold() {
    let old = document();
    let mut new = old.clone();
    new.pages[0].blocks[1] = block(2, "Payment is due on day sixty", 350.0, 300.0);
    for (threshold, changed) in [(0.5, 1), (0.99, 0)] {
        let options = DiffOptions {
            block_match_score: 0.99,
            text_change_score: threshold,
            ..DiffOptions::default()
        };
        let diff =
            try_diff_documents("old", "new", &old, &new, &Limits::default(), &options).unwrap();
        assert_eq!(diff.summary.blocks_changed, changed);
        assert_eq!(diff.summary.blocks_added, 1 - changed);
        assert_eq!(diff.summary.blocks_removed, 1 - changed);
        if changed == 1 {
            assert!(diff.pages[0]
                .blocks
                .iter()
                .any(|b| b.moved && b.old_index == Some(2)));
        }
    }
}

#[test]
fn block_match_and_position_recovery_have_distinct_effects() {
    let old = document();
    let mut new = old.clone();
    new.pages[0].blocks[1] = block(2, "Payment is due on day sixty", 350.0, 300.0);
    for (threshold, changed) in [(0.1, 1), (0.99, 0)] {
        let options = DiffOptions {
            block_match_score: threshold,
            text_change_score: 1.0,
            ..DiffOptions::default()
        };
        let result = diff_documents("a", "b", &old, &new, &options);
        assert_eq!(result.summary.blocks_changed, changed);
    }
    let mut old = document();
    old.pages[0].blocks[1].text = "111111".into();
    let mut new = old.clone();
    new.pages[0].blocks[1] = block(2, "222222", 150.0, 400.0);
    for (overlap, changed) in [(0.4, 1), (0.9, 0)] {
        let result = diff_documents(
            "a",
            "b",
            &old,
            &new,
            &DiffOptions {
                position_overlap: overlap,
                ..DiffOptions::default()
            },
        );
        assert_eq!(result.summary.blocks_changed, changed);
    }
}

#[test]
fn position_and_font_tolerances_control_their_reported_changes() {
    let old = document();
    let mut new = old.clone();
    let b = &mut new.pages[0].blocks[1];
    b.bounds.x0 += 0.5;
    b.bounds.x1 += 0.5;
    b.size = Some(12.1);
    for (tolerance, changes) in [(1.0, 0), (0.01, 1)] {
        let result = diff_documents(
            "a",
            "b",
            &old,
            &new,
            &DiffOptions {
                position_tolerance: tolerance,
                font_size_tolerance: tolerance,
                ..DiffOptions::default()
            },
        );
        assert_eq!(result.summary.blocks_moved, changes);
        assert_eq!(result.summary.blocks_format_changed, changes);
    }
}

#[test]
fn page_match_threshold_controls_unrelated_layout_candidates() {
    let old = document();
    let mut new = old.clone();
    new.pages[0].blocks[0].text = "This anchor paragraph identifies the edited document.".into();
    new.pages[0].blocks[1] = block(2, "A completely different paragraph", 350.0, 300.0);
    for (threshold, added) in [(0.1, 0), (1.0, 1)] {
        let result = diff_documents(
            "a",
            "b",
            &old,
            &new,
            &DiffOptions {
                page_match_score: threshold,
                ..DiffOptions::default()
            },
        );
        assert_eq!(result.summary.pages_added, added);
        assert_eq!(result.summary.pages_removed, added);
    }
}

#[test]
fn alignment_budget_never_forces_unscored_page_or_block_matches() {
    let mut old = document();
    old.pages[0].blocks.extend([
        block(3, "CCCC", 100.0, 200.0),
        block(4, "DDDD", 100.0, 150.0),
    ]);
    let mut new = old.clone();
    for (i, text) in ["1111", "2222", "3333"].iter().enumerate() {
        new.pages[0].blocks[i + 1].text = (*text).into();
    }
    let diff = diff_documents(
        "a",
        "b",
        &old,
        &new,
        &DiffOptions {
            max_pairwise: 1,
            ..DiffOptions::default()
        },
    );
    assert!(!diff.diagnostics.page_alignment_limited);
    assert_eq!(diff.diagnostics.block_alignment_limited_pages, 1);
    assert!(!diff.is_text_comparison_complete());
    assert_eq!(diff.summary.blocks_changed, 0);
    assert_eq!(diff.summary.blocks_added, 3);
    assert_eq!(diff.summary.blocks_removed, 3);

    let diff = diff_documents(
        "a",
        "b",
        &old,
        &new,
        &DiffOptions {
            max_pairwise: 0,
            ..DiffOptions::default()
        },
    );
    assert!(diff.diagnostics.page_alignment_limited);
    assert_eq!(diff.summary.pages_added, 1);
    assert_eq!(diff.summary.pages_removed, 1);
    assert!(!diff.is_text_comparison_complete());
    let identical = diff_documents(
        "a",
        "a",
        &old,
        &old,
        &DiffOptions {
            max_pairwise: 0,
            ..DiffOptions::default()
        },
    );
    assert!(!identical.has_changes());
    assert!(identical.is_text_comparison_complete());
}

#[test]
fn checked_entry_points_reject_nonfinite_and_out_of_range_options_before_pdf_io() {
    type SetOption = fn(&mut DiffOptions, f64);
    let scores: [SetOption; 4] = [
        |o, v| o.page_match_score = v,
        |o, v| o.block_match_score = v,
        |o, v| o.text_change_score = v,
        |o, v| o.position_overlap = v,
    ];
    for set in scores {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
            let mut options = DiffOptions::default();
            set(&mut options, invalid);
            assert!(options.validate().is_err());
            let error = pdf_diff::diff_bytes(
                "a",
                "b",
                b"invalid",
                b"invalid",
                &Limits::default(),
                &options,
            )
            .unwrap_err();
            assert!(error.to_string().contains("must be finite"));
        }
    }
    let tolerances: [SetOption; 2] = [
        |o, v| o.position_tolerance = v,
        |o, v| o.font_size_tolerance = v,
    ];
    for set in tolerances {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01] {
            let mut options = DiffOptions::default();
            set(&mut options, invalid);
            assert!(options.validate().is_err());
        }
    }
    let mut invalid = document();
    invalid.pages[0].blocks[1].bounds = Bounds {
        x0: -f64::MAX,
        x1: f64::MAX,
        y0: 0.0,
        y1: 1.0,
    };
    assert!(try_diff_documents(
        "a",
        "b",
        &invalid,
        &document(),
        &Limits::default(),
        &DiffOptions::default()
    )
    .is_err());
    for limits in [
        Limits {
            max_input_mb: 0,
            ..Limits::default()
        },
        Limits {
            max_pages: 0,
            ..Limits::default()
        },
        Limits {
            max_blocks_per_page: 0,
            ..Limits::default()
        },
    ] {
        assert!(limits.validate().is_err());
        assert!(try_diff_documents(
            "a",
            "a",
            &document(),
            &document(),
            &limits,
            &DiffOptions::default()
        )
        .is_err());
    }
}
