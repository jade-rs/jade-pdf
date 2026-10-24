#[path = "support/appearance_fixture.rs"]
mod appearance_fixture;

use pdf_diff::{diff_bytes, load_document_from_bytes, text_layer_from_bytes, DiffOptions, Limits};

fn without_order_review(mut value: serde_json::Value) -> serde_json::Value {
    fn clear(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                fields.remove("reading_order_requires_review");
                fields.remove("source_text_accounting_unavailable");
                fields.remove("source_unicode_accounting_unavailable");
                for child in fields.values_mut() {
                    clear(child);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    clear(child);
                }
            }
            _ => {}
        }
    }
    clear(&mut value);
    value
}

fn assert_geometric_fallback(tagged: &[u8], untagged: &[u8], label: &str) {
    let a = load_document_from_bytes(tagged, &Limits::default()).unwrap();
    let b = load_document_from_bytes(untagged, &Limits::default()).unwrap();
    assert!(a.pages[0].reading_order_requires_review, "{label}");
    assert!(!b.pages[0].reading_order_requires_review, "{label}");
    // A truncated structure walk cannot prove that no ActualText was hidden
    // below the depth cap. Preserve its independent source-accounting review.
    assert_eq!(
        a.pages[0].source_text_accounting_unavailable,
        label.starts_with("deep-tree"),
        "{label}"
    );
    assert_eq!(
        a.pages[0].source_unicode_accounting_unavailable,
        label.starts_with("deep-tree"),
        "{label}"
    );
    assert_eq!(
        without_order_review(serde_json::to_value(a).unwrap()),
        serde_json::to_value(b).unwrap(),
        "{label}"
    );
    let a = text_layer_from_bytes(tagged, 1).unwrap();
    let b = text_layer_from_bytes(untagged, 1).unwrap();
    assert!(a.iter().all(|r| r.reading_order_requires_review), "{label}");
    assert!(
        b.iter().all(|r| !r.reading_order_requires_review),
        "{label}"
    );
    assert_eq!(
        without_order_review(serde_json::to_value(a).unwrap()),
        serde_json::to_value(b).unwrap(),
        "{label}"
    );
}

#[test]
fn stale_spanning_headers_recover_visual_order_and_always_report_inference() {
    let expected="Heading across the table\nName\ncategory\nTotals\nQty\nPrice\nApple\n5\n10\nPear\n2\n6\nFooter across the table";
    for rtl in [false, true] {
        let reference = text_layer_from_bytes(
            &appearance_fixture::spanning_tagged_table_layout(1.0, 0, "complete", rtl, true, false),
            1,
        )
        .unwrap();
        assert!(reference.iter().all(|r| !r.reading_order_requires_review));
        for mode in ["stale-header", "stale-shuffled-rows"] {
            for unit in [1.0, 64.0, 72.0, 1024.0, 0.015625] {
                for angle in [0, 90, 180, 270] {
                    let bytes = appearance_fixture::spanning_tagged_table_layout(
                        unit, angle, mode, rtl, true, false,
                    );
                    let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                    assert_eq!(
                        doc.pages[0].text(),
                        expected,
                        "mode={mode}, unit={unit}, angle={angle}, rtl={rtl}"
                    );
                    assert!(doc.pages[0].reading_order_requires_review);
                    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                    assert_eq!(
                        layer
                            .iter()
                            .map(|r| r.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        expected
                    );
                    assert!(layer.iter().all(|r| r.reading_order_requires_review));
                    assert_eq!(reference.len(), layer.len());
                    for (a, b) in reference.iter().zip(&layer) {
                        assert_eq!(a.sdk_unicode_mapping_errors, b.sdk_unicode_mapping_errors);
                        assert_eq!(a.chars.len(), b.chars.len());
                        for (x, y) in [
                            (a.bounds.x0, b.bounds.x0),
                            (a.bounds.y0, b.bounds.y0),
                            (a.bounds.x1, b.bounds.x1),
                            (a.bounds.y1, b.bounds.y1),
                            (a.size, b.size),
                        ] {
                            assert!((x - y * unit).abs() < 0.001);
                        }
                        for (x, y) in a.chars.iter().zip(&b.chars) {
                            assert!((x.x0 - y.x0 * unit).abs() < 0.001);
                            assert!((x.x1 - y.x1 * unit).abs() < 0.001);
                        }
                    }
                }
            }
        }
    }
    let old = appearance_fixture::fixture("tagged_table_stale_edit", false);
    let new = appearance_fixture::fixture("tagged_table_stale_edit", true);
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
    assert!(!diff.is_text_comparison_complete());
    assert_eq!(diff.diagnostics.old_pages_with_uncertain_reading_order, 1);
    assert_eq!(diff.diagnostics.new_pages_with_uncertain_reading_order, 1);
    assert_eq!(
        diff.comparison
            .as_ref()
            .unwrap()
            .interactive_pages_requiring_review,
        0
    );
    assert_eq!(diff.summary.blocks_changed, 1);
    assert_eq!(diff.summary.pages_visual_changed, 1);
    let same = diff_bytes(
        "old",
        "same",
        &old,
        &old,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(same.is_comparison_complete());
    assert!(!same.is_text_comparison_complete());
}

#[test]
fn ambiguous_stale_spans_preserve_every_geometric_run_and_request_review() {
    for mode in [
        "stale-unmarked",
        "stale-two-orphans",
        "stale-other-content",
        "stale-whitespace",
        "stale-one-reference",
        "stale-bad-span",
        "stale-invalid-span",
        "stale-conflicting-span",
        "stale-wrong-owner",
        "stale-crossed-row",
    ] {
        for rtl in [false, true] {
            for angle in [0u16, 90, 180, 270] {
                let unit = if (angle / 90 + u16::from(rtl)) % 2 == 0 {
                    1.0
                } else {
                    64.0
                };
                let a = appearance_fixture::spanning_tagged_table_layout(
                    unit, angle, mode, rtl, true, false,
                );
                let b = appearance_fixture::spanning_tagged_table_layout(
                    unit, angle, mode, rtl, false, false,
                );
                assert_geometric_fallback(
                    &a,
                    &b,
                    &format!("{mode}, unit={unit}, angle={angle}, rtl={rtl}"),
                );
            }
        }
    }
}

#[test]
fn spanning_table_headers_preserve_rows_across_units_rotations_and_directions() {
    let expected = "Heading across the table\nName\ncategory\nTotals\nQty\nPrice\nApple\n5\n10\nPear\n2\n6\nFooter across the table";
    for rtl in [false, true] {
        let reference = text_layer_from_bytes(
            &appearance_fixture::spanning_tagged_table_layout(1.0, 0, "complete", rtl, true, false),
            1,
        )
        .unwrap();
        for mode in ["complete", "shuffled-rows"] {
            for unit in [1.0, 64.0, 72.0, 1024.0, 0.015625] {
                for angle in [0, 90, 180, 270] {
                    let bytes = appearance_fixture::spanning_tagged_table_layout(
                        unit, angle, mode, rtl, true, false,
                    );
                    let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                    assert_eq!(
                        doc.pages[0].text(),
                        expected,
                        "rtl={rtl}, mode={mode}, unit={unit}, angle={angle}"
                    );
                    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                    assert_eq!(
                        layer
                            .iter()
                            .map(|r| r.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        expected
                    );
                    assert_eq!(layer.len(), reference.len());
                    for (a, b) in reference.iter().zip(&layer) {
                        assert_eq!(a.chars.len(), b.chars.len());
                        for (x, y) in [
                            (a.bounds.x0, b.bounds.x0),
                            (a.bounds.y0, b.bounds.y0),
                            (a.bounds.x1, b.bounds.x1),
                            (a.bounds.y1, b.bounds.y1),
                            (a.size, b.size),
                        ] {
                            assert!((x - y * unit).abs() < 0.001);
                        }
                        for (x, y) in a.chars.iter().zip(&b.chars) {
                            assert!((x.x0 - y.x0 * unit).abs() < 0.001);
                            assert!((x.x1 - y.x1 * unit).abs() < 0.001);
                        }
                    }
                }
            }
        }
    }
    let old = appearance_fixture::fixture("tagged_table_spans_edit", false);
    let new = appearance_fixture::fixture("tagged_table_spans_edit", true);
    let diff = diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(diff.is_comparison_complete());
    assert_eq!(diff.summary.blocks_changed, 1);
    assert_eq!(diff.summary.pages_visual_changed, 1);
    assert_eq!(diff.summary.pages_added + diff.summary.pages_removed, 0);
}

#[test]
fn crossed_or_incomplete_spanning_rows_keep_exact_geometric_controls() {
    for mode in [
        "crossing-rows",
        "overlapping-cells",
        "contradictory-direction",
        "missing-cell",
        "foreign-text",
    ] {
        for rtl in [false, true] {
            for unit in [1.0, 64.0] {
                for angle in [0, 90, 180, 270] {
                    let a = appearance_fixture::spanning_tagged_table_layout(
                        unit, angle, mode, rtl, true, false,
                    );
                    let b = appearance_fixture::spanning_tagged_table_layout(
                        unit, angle, mode, rtl, false, false,
                    );
                    assert_geometric_fallback(
                        &a,
                        &b,
                        &format!("{mode}, unit={unit}, angle={angle}, rtl={rtl}"),
                    );
                }
            }
        }
    }
}

#[test]
fn complete_tagged_tables_follow_cell_order_inside_geometric_page_flow() {
    let expected = "A heading spanning both table columns\nItem\nPrice\nApples\n10\nA footer spanning both table columns";
    let reference = text_layer_from_bytes(
        &appearance_fixture::tagged_table_layout(1.0, 0, "complete", true, false),
        1,
    )
    .unwrap();
    for mode in ["complete", "shuffled-rows"] {
        for unit in [1.0, 64.0, 72.0, 1024.0, 0.015625] {
            for angle in [0, 90, 180, 270] {
                let bytes = appearance_fixture::tagged_table_layout(unit, angle, mode, true, false);
                let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(doc.pages[0].text(), expected, "unit={unit}, angle={angle}");
                let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                assert_eq!(
                    layer
                        .iter()
                        .map(|r| r.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    expected
                );
                assert_eq!(reference.len(), layer.len());
                for (a, b) in reference.iter().zip(&layer) {
                    assert_eq!(a.chars.len(), b.chars.len());
                    for (x, y) in [
                        (a.bounds.x0, b.bounds.x0),
                        (a.bounds.y0, b.bounds.y0),
                        (a.bounds.x1, b.bounds.x1),
                        (a.bounds.y1, b.bounds.y1),
                        (a.size, b.size),
                    ] {
                        assert!((x - y * unit).abs() < 0.001);
                    }
                    for (x, y) in a.chars.iter().zip(&b.chars) {
                        assert!((x.x0 - y.x0 * unit).abs() < 0.001);
                        assert!((x.x1 - y.x1 * unit).abs() < 0.001);
                    }
                }
            }
        }
    }
}

#[test]
fn partial_ambiguous_or_form_scoped_tags_keep_every_geometric_run() {
    for mode in [
        "missing-cell",
        "duplicate-id",
        "shared-cell",
        "visible-row-text",
        "form-collision",
        "deep-tree",
        "overlapping-rows",
        "contradictory-columns",
    ] {
        for unit in [1.0, 64.0] {
            for angle in [0, 90, 180, 270] {
                let a = appearance_fixture::tagged_table_layout(unit, angle, mode, true, false);
                let b = appearance_fixture::tagged_table_layout(unit, angle, mode, false, false);
                assert_geometric_fallback(&a, &b, &format!("{mode}, unit={unit}, angle={angle}"));
            }
        }
    }
}

#[test]
fn tounicode_only_edits_are_detected_with_identical_painted_glyphs() {
    let old = appearance_fixture::fixture("tounicode_override_edit", false);
    let new = appearance_fixture::fixture("tounicode_override_edit", true);
    let diff = diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(diff.is_comparison_complete());
    assert_eq!(diff.summary.pages_visual_changed, 0);
    assert_eq!(diff.summary.blocks_changed, 1);
    let change = &diff.pages[0].blocks[0];
    assert_eq!(change.old_text.as_deref(), Some("ó€éê"));
    assert_eq!(change.new_text.as_deref(), Some("ò€éê"));
    let old_file = tempfile::NamedTempFile::new().unwrap();
    let new_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(old_file.path(), old).unwrap();
    std::fs::write(new_file.path(), new).unwrap();
    let files = pdf_diff::diff_files(
        old_file.path(),
        new_file.path(),
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(files.summary).unwrap(),
        serde_json::to_value(diff.summary).unwrap()
    );
}

#[test]
fn later_tounicode_entries_override_earlier_ranges_in_all_text_routes() {
    let source = appearance_fixture::fixture("tounicode_overrides", false);
    let rewritten = appearance_fixture::fixture("tounicode_overrides", true);
    // Adobe TN5014 section 5.2: later mappings supersede earlier mappings.
    let expected = "ó€éê";
    let ir = load_document_from_bytes(&source, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text(), expected);
    assert_eq!(ir.pages[0].sdk_unicode_mapping_errors, 0);
    assert_eq!(
        text_layer_from_bytes(&source, 1)
            .unwrap()
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>(),
        expected
    );
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), &source).unwrap();
    assert_eq!(
        pdf_diff::load_document(file.path(), &Limits::default())
            .unwrap()
            .pages[0]
            .text(),
        expected
    );
    assert_eq!(
        pdf_diff::text_layer(file.path(), 1)
            .unwrap()
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>(),
        expected
    );
    let diff = diff_bytes(
        "old",
        "new",
        &source,
        &rewritten,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!diff.has_changes());
    assert!(diff.is_comparison_complete());
}

fn pdf(stream: &str, rotation: u16) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Rotate {rotation} /Resources << /Font << /F1 4 0 R /F2 5 0 R >> >> /Contents 6 0 R >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
    ];
    let mut source = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
    }
    let xref = source.len();
    source.push_str("xref\n0 7\n0000000000 65535 f \n");
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    source.into_bytes()
}

#[test]
fn independently_painted_ltr_fragments_follow_their_geometric_order() {
    let stream = "BT /F1 12 Tf 28 200 Td (m) Tj ET
BT /F1 12 Tf 20 200 Td (1) Tj ET
BT /F1 12 Tf 120 200 Td (663) Tj ET
BT /F1 12 Tf 116 200 Td (.) Tj ET
BT /F1 12 Tf 100 200 Td (30) Tj ET
BT /F1 12 Tf 90 200 Td (=) Tj ET
BT /F1 12 Tf 145 200 Td (319) Tj ET";
    for rotation in [0, 90, 180, 270] {
        let doc = load_document_from_bytes(&pdf(stream, rotation), &Limits::default()).unwrap();
        let compact: String = doc.pages[0]
            .text()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert_eq!(compact, "1m=30.663319", "rotation={rotation}");
    }
}

#[test]
fn inline_scripts_stay_between_their_surrounding_words() {
    let stream = "BT /F1 12 Tf 20 200 Td (A) Tj ET
BT /F1 7 Tf 28 206 Td (2) Tj ET
BT /F1 12 Tf 34 200 Td ( + B) Tj ET
BT /F1 7 Tf 60 197 Td (i) Tj ET
BT /F1 12 Tf 64 200 Td ( = C) Tj ET
BT /F1 12 Tf 20 220 Td (m) Tj ET
BT /F1 7 Tf 30 226 Td (2) Tj ET
BT /F1 12 Tf 37 220 Td (s) Tj ET
BT /F1 7 Tf 43 226 Td (-2) Tj ET
BT /F1 12 Tf 20 180 Td (Next line) Tj ET";
    for rotation in [0, 90, 180, 270] {
        let source = pdf(stream, rotation);
        let doc = load_document_from_bytes(&source, &Limits::default()).unwrap();
        let layer = text_layer_from_bytes(&source, 1).unwrap();
        for text in [
            doc.pages[0].text(),
            layer.iter().map(|r| r.text.as_str()).collect::<String>(),
        ] {
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            assert_eq!(
                compact, "m2s-2A2+Bi=CNextline",
                "rotation={rotation}: {text}"
            );
        }
    }
}

#[test]
fn nearby_heading_and_body_baselines_remain_separate_lines() {
    let stream = "BT /F1 14 Tf 100 240 Td (Section) Tj ET
BT /F1 12 Tf 20 234 Td (Body follows this short centered heading) Tj ET
BT /F1 12 Tf 100 160 Td (Top) Tj ET
BT /F1 12 Tf 20 154 Td (Bottom) Tj ET";
    for rotation in [0, 90, 180, 270] {
        let doc = load_document_from_bytes(&pdf(stream, rotation), &Limits::default()).unwrap();
        assert_eq!(
            doc.pages[0].text(),
            "Section\nBody follows this short centered heading\nTop\nBottom",
            "rotation={rotation}"
        );
    }
}

#[test]
fn bullet_markers_remain_with_their_items_beside_another_column() {
    let stream = "BT /F1 10 Tf 20 200 Td (\\267) Tj ET
BT /F1 10 Tf 42 200 Td (First) Tj ET
BT /F1 10 Tf 20 180 Td (\\267) Tj ET
BT /F1 10 Tf 42 180 Td (Second) Tj ET
BT /F1 10 Tf 170 200 Td (Other column) Tj ET
BT /F1 10 Tf 170 180 Td (Other next) Tj ET";
    for rotation in [0, 90, 180, 270] {
        let source = pdf(stream, rotation);
        let doc = load_document_from_bytes(&source, &Limits::default()).unwrap();
        let text: String = doc.pages[0]
            .text()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert_eq!(
            text, "•First•SecondOthercolumnOthernext",
            "rotation={rotation}"
        );
    }
}

#[test]
fn changing_column_counts_restart_the_reading_region() {
    let mut stream =
        String::from("BT /F1 12 Tf 10 290 Td (Title spans all author columns and regions) Tj ET\n");
    for (names, xs, y) in [
        (
            vec!["Aone", "Atwo", "Athree", "Afour"],
            vec![10, 80, 150, 220],
            230,
        ),
        (vec!["Bone", "Btwo", "Bthree"], vec![10, 110, 210], 170),
    ] {
        for (name, x) in names.into_iter().zip(xs) {
            for (line, text) in [(0, name), (10, "Org"), (20, "mail")] {
                stream.push_str(&format!("BT /F1 8 Tf {x} {} Td ({text}) Tj ET\n", y - line));
            }
        }
    }
    let assert_order = |text: &str, anchors: &[&str]| {
        let mut offset = 0;
        for anchor in anchors {
            let at = text[offset..]
                .find(anchor)
                .unwrap_or_else(|| panic!("Missing ordered anchor {anchor}: {text}"));
            offset += at + anchor.len();
        }
    };
    for rotation in [0, 90, 180, 270] {
        let source = pdf(&stream, rotation);
        let doc = load_document_from_bytes(&source, &Limits::default()).unwrap();
        let text = doc.pages[0].text();
        let layer = text_layer_from_bytes(&source, 1)
            .unwrap()
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for actual in [&text, &layer] {
            assert_order(
                actual,
                &["Aone", "Atwo", "Athree", "Afour", "Bone", "Btwo", "Bthree"],
            );
        }
        // A shared paragraph break in two persistent columns must not switch
        // to row-major reading merely because a horizontal gap exists.
        let same=pdf("BT /F1 10 Tf 10 240 Td (Left first) Tj ET BT /F1 10 Tf 10 160 Td (Left second) Tj ET BT /F1 10 Tf 170 240 Td (Right first) Tj ET BT /F1 10 Tf 170 160 Td (Right second) Tj ET",rotation);
        let doc = load_document_from_bytes(&same, &Limits::default()).unwrap();
        assert_order(
            &doc.pages[0].text(),
            &["Left first", "Left second", "Right first", "Right second"],
        );
        let partial=pdf("BT /F1 10 Tf 10 240 Td (A end) Tj ET BT /F1 10 Tf 110 240 Td (B top) Tj ET BT /F1 10 Tf 210 240 Td (C top) Tj ET BT /F1 10 Tf 110 160 Td (B bottom) Tj ET BT /F1 10 Tf 210 160 Td (C bottom) Tj ET",rotation);
        let doc = load_document_from_bytes(&partial, &Limits::default()).unwrap();
        assert_order(
            &doc.pages[0].text(),
            &["A end", "B top", "B bottom", "C top", "C bottom"],
        );
    }
}

#[test]
fn overlapping_text_objects_do_not_duplicate_characters() {
    let bytes = pdf(
        "BT /F1 12 Tf 20 100 Td (ABC) Tj ET\nBT /F2 12 Tf 25 100 Td (123) Tj ET",
        0,
    );
    let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    let mut chars: Vec<_> = doc.pages[0]
        .text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    chars.sort_unstable();
    assert_eq!(chars, vec!['1', '2', '3', 'A', 'B', 'C']);
    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
    assert_eq!(
        layer
            .iter()
            .map(|r| r.text.chars().filter(|c| !c.is_whitespace()).count())
            .sum::<usize>(),
        6
    );
}

#[test]
fn inline_font_objects_and_punctuation_are_stable_after_page_rotation() {
    let stream = "BT /F2 12 Tf 20 200 Td (Hello) Tj ET\nBT /F1 12 Tf 52 200 Td ( world!) Tj ET\nBT /F1 12 Tf 20 180 Td (Next line) Tj ET";
    let old = pdf(stream, 0);
    let new = pdf(stream, 90);
    let diff = diff_bytes(
        "a",
        "b",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(diff.summary.pages_geometry_changed, 1);
    assert_eq!(diff.summary.blocks_changed, 0, "{diff:?}");
    assert_eq!(diff.summary.blocks_added, 0);
    assert_eq!(diff.summary.blocks_removed, 0);
    let doc = load_document_from_bytes(&old, &Limits::default()).unwrap();
    assert!(doc.pages[0].text().contains("Hello world!"));
    // Separate font faces must reach the same spacing canonicalizer whether
    // the SDK visits the numeric or italic/bold object first after rotation.
    for rotation in [0, 90, 180, 270] {
        let source = pdf(
            "BT /F1 12 Tf 20 100 Td (1) Tj ET BT /F2 12 Tf 30 100 Td (m) Tj ET",
            rotation,
        );
        let doc = load_document_from_bytes(&source, &Limits::default()).unwrap();
        assert_eq!(doc.pages[0].text(), "1 m", "rotation={rotation}");
    }
}

#[test]
fn a_page_containing_only_a_replaced_number_remains_one_modified_page() {
    let old = pdf("BT /F1 24 Tf 100 100 Td (111111) Tj ET", 0);
    let new = pdf("BT /F1 24 Tf 100 100 Td (222222) Tj ET", 0);
    let diff = diff_bytes(
        "a",
        "b",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(diff.summary.pages_modified, 1);
    assert_eq!(diff.summary.pages_added, 0);
    assert_eq!(diff.summary.pages_removed, 0);
    assert_eq!(diff.summary.blocks_changed, 1);
    assert_eq!(diff.summary.blocks_format_changed, 0);
    // A caller requesting strict text similarity can disable the weak layout candidate.
    let strict = DiffOptions {
        page_match_score: 0.9,
        ..DiffOptions::default()
    };
    let diff = diff_bytes("a", "b", &old, &new, &Limits::default(), &strict).unwrap();
    assert_eq!(diff.summary.pages_added, 1);
    assert_eq!(diff.summary.pages_removed, 1);
}

#[test]
fn multi_column_reading_order_keeps_the_left_column_together() {
    let stream="BT /F1 12 Tf 20 250 Td (Left title) Tj ET\nBT /F1 12 Tf 20 230 Td (continues here) Tj ET\nBT /F1 12 Tf 180 250 Td (Right title) Tj ET\nBT /F1 12 Tf 180 230 Td (right body) Tj ET";
    let doc = load_document_from_bytes(&pdf(stream, 0), &Limits::default()).unwrap();
    assert_eq!(
        doc.pages[0].text(),
        "Left title\ncontinues here\nRight title\nright body"
    );
}

#[test]
fn spanning_heading_and_footer_do_not_interleave_body_columns() {
    let stream = "BT /F1 12 Tf 20 270 Td (A heading spanning the two columns) Tj ET
BT /F1 12 Tf 20 200 Td (Left first) Tj ET
BT /F1 12 Tf 180 200 Td (Right first) Tj ET
BT /F1 12 Tf 20 180 Td (Left last) Tj ET
BT /F1 12 Tf 180 180 Td (Right last) Tj ET
BT /F1 12 Tf 20 100 Td (A footer spanning the two columns) Tj ET";
    let bytes = pdf(stream, 0);
    let expected = "A heading spanning the two columns\nLeft first\nLeft last\nRight first\nRight last\nA footer spanning the two columns";
    let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(doc.pages[0].text(), expected);
    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
    assert_eq!(
        layer
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        expected
    );
    // Page display rotation must not change raw-space reading order.
    for angle in [90, 180, 270] {
        let rotated = load_document_from_bytes(&pdf(stream, angle), &Limits::default()).unwrap();
        assert_eq!(rotated.pages[0].text(), expected, "rotation {angle}");
    }
}

#[test]
fn spanning_subheading_restarts_columns_in_the_next_region() {
    let stream = "BT /F1 10 Tf 20 270 Td (Upper left) Tj ET
BT /F1 10 Tf 180 270 Td (Upper right) Tj ET
BT /F1 10 Tf 20 250 Td (Left ends) Tj ET
BT /F1 10 Tf 180 250 Td (Right ends) Tj ET
BT /F1 12 Tf 20 200 Td (Section spanning the two columns) Tj ET
BT /F1 10 Tf 20 150 Td (Lower left) Tj ET
BT /F1 10 Tf 180 150 Td (Lower right) Tj ET
BT /F1 10 Tf 20 130 Td (Left finishes) Tj ET
BT /F1 10 Tf 180 130 Td (Right finishes) Tj ET";
    let doc = load_document_from_bytes(&pdf(stream, 0), &Limits::default()).unwrap();
    assert_eq!(doc.pages[0].text(), "Upper left\nLeft ends\nUpper right\nRight ends\nSection spanning the two columns\nLower left\nLeft finishes\nLower right\nRight finishes");
}

#[test]
fn narrow_column_gutters_do_not_join_unrelated_lines() {
    let stream = "BT /F1 12 Tf 20 280 Td (Heading across both columns) Tj ET
BT /F1 10 Tf 20 250 Td (WWWWWWWWWW) Tj ET
BT /F1 10 Tf 124 250 Td (Right first) Tj ET
BT /F1 10 Tf 20 230 Td (Left last) Tj ET
BT /F1 10 Tf 124 230 Td (Right last) Tj ET";
    let doc = load_document_from_bytes(&pdf(stream, 0), &Limits::default()).unwrap();
    assert_eq!(
        doc.pages[0].text(),
        "Heading across both columns\nWWWWWWWWWW\nLeft last\nRight first\nRight last"
    );
}

#[test]
fn explicit_word_spacing_is_not_mistaken_for_a_column_boundary() {
    let bytes = pdf("BT /F1 12 Tf 20 200 Td (A    B) Tj ET", 0);
    let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    // Pdfium normalizes the encoded spaces to one space, not a new paragraph.
    assert_eq!(doc.pages[0].text(), "A B");
    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
    assert_eq!(layer.len(), 1);
    assert_eq!(layer[0].text, "A B");
}

#[test]
fn form_dot_leaders_remain_attached_after_display_rotation() {
    let stream = "BT /F1 9 Tf 20 200 Td (Caption) Tj ET
BT /F1 9 Tf 62 200 Td (. . . . . .) Tj ET";
    for angle in [0, 90, 180, 270] {
        let bytes = pdf(stream, angle);
        let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        assert_eq!(
            doc.pages[0].text(),
            "Caption . . . . . .",
            "rotation {angle}"
        );
        assert_eq!(text_layer_from_bytes(&bytes, 1).unwrap().len(), 1);
    }
}

#[test]
fn coordinate_units_preserve_lines_columns_and_text_layer_order() {
    let expected = "A heading spanning the two columns\nLeft first\nLeft last\nRight first\nRight last\nA footer spanning the two columns";
    let baseline =
        text_layer_from_bytes(&appearance_fixture::rescaled_layout(1.0, 0, false), 1).unwrap();
    for unit in [1.0, 64.0, 72.0, 1024.0, 0.015625] {
        for angle in [0, 90, 180, 270] {
            let bytes = appearance_fixture::rescaled_layout(unit, angle, false);
            let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
            assert_eq!(
                ir.pages[0].text(),
                expected,
                "unit {unit}, rotation {angle}"
            );
            let layer = text_layer_from_bytes(&bytes, 1).unwrap();
            assert_eq!(
                layer
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                expected
            );
            assert!(layer.iter().all(|r| (r.size * unit - 12.0).abs() < 0.001));
            for (old, new) in baseline.iter().zip(&layer) {
                for (a, b) in [
                    (old.bounds.x0, new.bounds.x0),
                    (old.bounds.y0, new.bounds.y0),
                    (old.bounds.x1, new.bounds.x1),
                    (old.bounds.y1, new.bounds.y1),
                ] {
                    assert!((a - b * unit).abs() < 0.001);
                }
                assert_eq!(old.chars.len(), new.chars.len());
                for (a, b) in old.chars.iter().zip(&new.chars) {
                    assert!((a.x0 - b.x0 * unit).abs() < 0.001);
                    assert!((a.x1 - b.x1 * unit).abs() < 0.001);
                }
            }
        }
    }
}

#[test]
fn distinct_construction_pieces_survive_physical_coordinate_normalization() {
    let baseline =
        text_layer_from_bytes(&appearance_fixture::construction_pieces(1.0, 0, false), 1).unwrap();
    for nested in [false, true] {
        for unit in [1.0, 64.0, 72.0, 1024.0, 0.015625] {
            for rotation in [0, 90, 180, 270] {
                let bytes = appearance_fixture::construction_pieces(unit, rotation, nested);
                let original = bytes.clone();
                let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(doc.pages[0].user_unit, Some(unit));
                assert_eq!(doc.pages[1].user_unit, Some(1.0));
                for number in [1, 2, 1] {
                    let layer = text_layer_from_bytes(&bytes, number).unwrap();
                    let text: String = layer
                        .iter()
                        .flat_map(|r| r.text.chars())
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    assert_eq!(
                        text, "||",
                        "unit={unit} rotation={rotation} nested={nested} page={number}"
                    );
                    assert_eq!(
                        doc.pages[number - 1]
                            .text()
                            .split_whitespace()
                            .collect::<String>(),
                        "||"
                    );
                    assert_eq!(doc.pages[number - 1].sdk_unicode_mapping_errors, 0);
                    let scale = if number == 1 { unit } else { 1.0 };
                    assert_eq!(baseline.len(), layer.len());
                    for (a, b) in baseline.iter().zip(&layer) {
                        for (x, y) in [
                            (a.bounds.x0, b.bounds.x0),
                            (a.bounds.y0, b.bounds.y0),
                            (a.bounds.x1, b.bounds.x1),
                            (a.bounds.y1, b.bounds.y1),
                            (a.size, b.size),
                        ] {
                            assert!(
                                (x - y * scale).abs() < 0.001,
                                "physical geometry {x} vs {}",
                                y * scale
                            );
                        }
                        assert_eq!(a.chars.len(), b.chars.len());
                        for (x, y) in a.chars.iter().zip(&b.chars) {
                            assert!((x.x0 - y.x0 * scale).abs() < 0.001);
                            assert!((x.x1 - y.x1 * scale).abs() < 0.001);
                        }
                    }
                }
                assert_eq!(bytes, original);
            }
        }
    }
}

#[test]
fn normalized_text_queries_leave_comparison_rendering_in_original_coordinates() {
    let old = appearance_fixture::construction_pieces(1.0, 90, true);
    let new = appearance_fixture::construction_pieces(64.0, 90, true);
    let limits = Limits::default();
    let options = DiffOptions::default();
    let bytes = diff_bytes("old", "new", &old, &new, &limits, &options).unwrap();
    assert_eq!(bytes.summary.pages_geometry_changed, 1);
    assert_eq!(bytes.summary.blocks_changed, 0);
    assert_eq!(bytes.summary.blocks_moved, 0);
    assert_eq!(bytes.summary.blocks_resized, 0);
    assert_eq!(bytes.summary.blocks_format_changed, 0);
    assert!(bytes.is_comparison_complete());
    assert_eq!(bytes.summary.pages_visual_changed, 0);
    let a = tempfile::NamedTempFile::new().unwrap();
    let b = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(a.path(), old).unwrap();
    std::fs::write(b.path(), new).unwrap();
    let files = pdf_diff::diff_files(a.path(), b.path(), &limits, &options).unwrap();
    assert_eq!(files.summary.pages_visual_changed, 0);
    assert!(files.is_comparison_complete());
    assert_eq!(
        serde_json::to_value(bytes.summary).unwrap(),
        serde_json::to_value(files.summary).unwrap()
    );
}

#[test]
fn diagonal_text_positions_do_not_imply_parallel_columns() {
    let stream = "BT /F1 10 Tf 66 90 Td (A1) Tj ET
BT /F1 10 Tf 38 60 Td (A2) Tj ET
BT /F1 10 Tf 60 130 Td (A3) Tj ET";
    let ir = load_document_from_bytes(&pdf(stream, 0), &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text(), "A3\nA1\nA2");
}

#[test]
fn generated_word_space_geometry_scales_with_its_neighboring_glyphs() {
    let a = text_layer_from_bytes(&appearance_fixture::spaced_layout(1.0), 1).unwrap();
    let b = text_layer_from_bytes(&appearance_fixture::spaced_layout(64.0), 1).unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].text, "Alpha Beta");
    assert_eq!(b[0].text, a[0].text);
    for (x, y) in a[0].chars.iter().zip(&b[0].chars) {
        assert!((x.x0 - y.x0 * 64.0).abs() < 0.001);
        assert!((x.x1 - y.x1 * 64.0).abs() < 0.001);
    }
}

#[test]
fn printed_line_end_hyphens_are_preserved_in_text_and_highlights() {
    let source = pdf(
        "BT /F1 12 Tf 20 220 Td (well-) Tj 0 -16 Td (known) Tj ET",
        0,
    );
    let doc = load_document_from_bytes(&source, &Limits::default()).unwrap();
    assert_eq!(doc.pages[0].text(), "well-\nknown");
    let layer = pdf_diff::text_layer_from_bytes(&source, 1).unwrap();
    assert!(layer.iter().any(|run| run.text == "well-"));
}

#[test]
fn replacement_text_is_reported_as_incomplete_even_when_both_sides_match() {
    let mut doc = load_document_from_bytes(
        &pdf("BT /F1 12 Tf 20 100 Td (ABC) Tj ET", 0),
        &Limits::default(),
    )
    .unwrap();
    doc.pages[0].blocks[0].text = "Unreadable \u{fffd}".into();
    let diff = pdf_diff::diff_documents("a", "a", &doc, &doc, &DiffOptions::default());
    assert!(!diff.has_changes());
    assert!(!diff.is_text_comparison_complete());
    assert!(diff.diagnostics.has_unmapped_text());
    assert_eq!(diff.diagnostics.old_pages_with_unmapped_text, 1);
    assert_eq!(diff.diagnostics.new_pages_with_unmapped_text, 1);
}

#[test]
fn sdk_unicode_mapping_errors_are_propagated_without_losing_printable_fallbacks() {
    for case in [
        "mapping_error_printable",
        "mapping_error_control",
        "mapping_error_mixed",
    ] {
        let expected = if case == "mapping_error_control" {
            "\u{fffd}"
        } else if case == "mapping_error_mixed" {
            "Aok"
        } else {
            "A"
        };
        let a = appearance_fixture::fixture(case, false);
        let b = appearance_fixture::fixture(case, true);
        let doc = load_document_from_bytes(&a, &Limits::default()).unwrap();
        assert_eq!(
            doc.pages[0].text().split_whitespace().collect::<String>(),
            expected,
            "{case}"
        );
        assert_eq!(doc.pages[0].sdk_unicode_mapping_errors, 1, "{case}");
        let layer = pdf_diff::text_layer_from_bytes(&a, 1).unwrap();
        assert_eq!(
            layer
                .iter()
                .map(|r| r.sdk_unicode_mapping_errors)
                .sum::<usize>(),
            1,
            "{case}"
        );
        assert_eq!(
            layer
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>()
                .split_whitespace()
                .collect::<String>(),
            expected,
            "{case}"
        );
        if case == "mapping_error_mixed" {
            assert_eq!(
                layer.len(),
                1,
                "Mixed font runs must merge without losing the SDK flag"
            );
        }
        let roundtrip: pdf_diff::DocumentIr =
            serde_json::from_slice(&serde_json::to_vec(&doc).unwrap()).unwrap();
        let ir_diff = pdf_diff::try_diff_documents(
            "ir",
            "ir",
            &roundtrip,
            &roundtrip,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            !ir_diff.is_text_comparison_complete(),
            "IR serialization lost the mapping flag"
        );
        let diff = pdf_diff::diff_bytes(
            "a",
            "b",
            &a,
            &b,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(!diff.has_changes(), "{case}");
        assert!(!diff.is_text_comparison_complete(), "{case}");
        assert!(!diff.is_comparison_complete(), "{case}");
        assert_eq!(diff.diagnostics.old_pages_with_unmapped_text, 1);
        assert_eq!(diff.diagnostics.new_pages_with_unmapped_text, 1);
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old.pdf");
        let new = directory.path().join("new.pdf");
        std::fs::write(&old, &a).unwrap();
        std::fs::write(&new, &b).unwrap();
        assert_eq!(
            pdf_diff::load_document_blocking(&old, &Limits::default())
                .unwrap()
                .pages[0]
                .text()
                .split_whitespace()
                .collect::<String>(),
            expected
        );
        assert_eq!(
            pdf_diff::text_layer(&old, 1).unwrap()[0]
                .text
                .split_whitespace()
                .collect::<String>(),
            expected
        );
        let file_diff =
            pdf_diff::diff_files(&old, &new, &Limits::default(), &DiffOptions::default()).unwrap();
        assert!(!file_diff.is_comparison_complete());
        let cli = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
            .arg(&old)
            .arg(&new)
            .output()
            .unwrap();
        assert_eq!(cli.status.code(), Some(3));
        let result: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
        assert_eq!(result["diagnostics"]["old_pages_with_unmapped_text"], 1);
        assert!(pdf_diff::diff_bytes(
            "a",
            "a",
            &a,
            &a,
            &Limits::default(),
            &DiffOptions::default()
        )
        .unwrap()
        .is_comparison_complete());
    }
}

#[test]
fn horizontal_shear_preserves_tagged_cell_order_across_units_and_rotations() {
    let expected="A heading spanning both table columns\nItem\nPrice\nApples\n10\nA footer spanning both table columns";
    for mode in ["shear-positive", "shear-negative"] {
        let reference = text_layer_from_bytes(
            &appearance_fixture::tagged_table_layout(1.0, 0, mode, true, false),
            1,
        )
        .unwrap();
        for unit in [1.0, 64.0, 72.0, 1024.0, 1.0 / 64.0] {
            for rotation in [0, 90, 180, 270] {
                let bytes =
                    appearance_fixture::tagged_table_layout(unit, rotation, mode, true, false);
                let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(doc.pages[0].text(), expected, "{mode}/{unit}/{rotation}");
                assert!(!doc.pages[0].reading_order_requires_review);
                let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                assert_eq!(
                    layer
                        .iter()
                        .map(|x| x.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    expected
                );
                assert_eq!(reference.len(), layer.len());
                for (a, b) in reference.iter().zip(&layer) {
                    assert_eq!(a.chars.len(), b.chars.len());
                    for (x, y) in [
                        (a.bounds.x0, b.bounds.x0),
                        (a.bounds.y0, b.bounds.y0),
                        (a.bounds.x1, b.bounds.x1),
                        (a.bounds.y1, b.bounds.y1),
                        (a.size, b.size),
                    ] {
                        assert!((x - y * unit).abs() < 0.001, "{mode}/{unit}/{rotation}");
                    }
                    for (x, y) in a.chars.iter().zip(&b.chars) {
                        assert!((x.x0 - y.x0 * unit).abs() < 0.001);
                        assert!((x.x1 - y.x1 * unit).abs() < 0.001);
                    }
                }
            }
        }
    }
}

#[test]
fn actual_sloping_baselines_keep_tagged_table_uncertainty_and_geometric_content() {
    for unit in [1.0, 64.0, 72.0, 1024.0, 1.0 / 64.0] {
        for rotation in [0, 90, 180, 270] {
            let a = appearance_fixture::tagged_table_layout(
                unit,
                rotation,
                "baseline-diagonal",
                true,
                false,
            );
            let b = appearance_fixture::tagged_table_layout(
                unit,
                rotation,
                "baseline-diagonal",
                false,
                false,
            );
            assert_geometric_fallback(&a, &b, &format!("diagonal/{unit}/{rotation}"));
        }
    }
}

#[test]
fn sheared_inline_glyphs_stay_between_their_horizontal_neighbors() {
    let stream="BT /F1 12 Tf 1 0 0 1 20 200 Tm (A) Tj ET\nBT /F1 12 Tf 1 0 0.333333 1 30 200 Tm (B) Tj ET\nBT /F1 12 Tf 1 0 0 1 40 200 Tm (C) Tj ET";
    for rotation in [0, 90, 180, 270] {
        let source = pdf(stream, rotation);
        let doc = load_document_from_bytes(&source, &Limits::default()).unwrap();
        assert_eq!(
            doc.pages[0].text().split_whitespace().collect::<String>(),
            "ABC"
        );
        let layer = text_layer_from_bytes(&source, 1).unwrap();
        assert_eq!(
            layer
                .iter()
                .flat_map(|r| r.text.chars())
                .filter(|c| !c.is_whitespace())
                .collect::<String>(),
            "ABC"
        );
    }
}

#[test]
fn supplementary_unicode_expansions_have_one_span_per_scalar_and_preserve_declared_repeats() {
    for unit in [1.0, 64.0, 72.0, 1024.0, 1.0 / 64.0] {
        for rotation in [0, 90, 180, 270] {
            let bytes = appearance_fixture::supplementary_unicode(unit, rotation, false);
            let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
            assert_eq!(doc.pages[0].text(), "😀𝑐𝑐C", "{unit}/{rotation}");
            assert_eq!(doc.pages[0].sdk_unicode_mapping_errors, 0);
            let layer = text_layer_from_bytes(&bytes, 1).unwrap();
            assert_eq!(
                layer.iter().map(|r| r.text.chars().count()).sum::<usize>(),
                4
            );
            assert_eq!(layer.iter().map(|r| r.chars.len()).sum::<usize>(), 4);
            assert!(layer
                .iter()
                .all(|r| r.text.chars().count() == r.chars.len()));
        }
    }
    let space = appearance_fixture::supplementary_unicode_mode(1.0, 0, "invisible");
    let doc = load_document_from_bytes(&space, &Limits::default()).unwrap();
    assert_eq!(doc.pages[0].text(), "😀");
    let a = appearance_fixture::fixture("supplementary_unicode_edit", false);
    let b = appearance_fixture::fixture("supplementary_unicode_edit", true);
    let diff = pdf_diff::diff_bytes(
        "old",
        "new",
        &a,
        &b,
        &Limits::default(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(diff.summary.blocks_changed, 1);
    assert_eq!(diff.summary.pages_visual_changed, 0);
    assert!(diff.is_comparison_complete());
}

#[test]
fn unmatched_surrogates_do_not_pair_across_distinct_glyphs_or_text_objects() {
    for (mode, expected) in [
        ("split", "��C"),
        ("coincident", "��C"),
        ("lone-high", "�BC"),
        ("lone-low", "�BC"),
        ("bad-order", "��C"),
    ] {
        for rotation in [0, 90, 180, 270] {
            let bytes = appearance_fixture::supplementary_unicode_mode(1.0, rotation, mode);
            let doc = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
            assert_eq!(
                doc.pages[0].text().split_whitespace().collect::<String>(),
                expected,
                "{mode}/{rotation}"
            );
            let layer = text_layer_from_bytes(&bytes, 1).unwrap();
            assert!(layer
                .iter()
                .all(|r| r.text.chars().count() == r.chars.len()));
            let mut rewritten = bytes.clone();
            rewritten.extend_from_slice(b"\n% rewrite\n");
            let diff = pdf_diff::diff_bytes(
                "old",
                "new",
                &bytes,
                &rewritten,
                &Limits::default(),
                &Default::default(),
            )
            .unwrap();
            assert!(!diff.is_comparison_complete());
        }
    }
    let old = appearance_fixture::fixture("surrogate_unpaired", false);
    let new = appearance_fixture::fixture("surrogate_unpaired", true);
    assert!(!pdf_diff::diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &Limits::default(),
        &Default::default()
    )
    .unwrap()
    .is_comparison_complete());
}
