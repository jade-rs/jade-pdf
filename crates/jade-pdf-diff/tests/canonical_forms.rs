#[path = "support/appearance_fixture.rs"]
mod appearance_fixture;
use pdf_diff::{diff_bytes, DiffOptions, Limits};

fn pdf(field: &str) -> Vec<u8> {
    let stream = "BT /F1 12 Tf 30 100 Td (Stable page) Tj ET";
    let objects=[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream",stream.len()),
        field.into(),
    ];
    let mut source = "%PDF-1.7\n".to_owned();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let start = source.len();
    source.push_str("xref\n0 7\n0000000000 65535 f \n");
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n"
    ));
    source.into_bytes()
}

fn compare(old: &[u8], new: &[u8]) -> pdf_diff::DocumentDiff {
    diff_bytes(
        "old",
        "new",
        old,
        new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap()
}

#[test]
fn unplaced_field_values_are_document_changes_without_invented_page_bounds() {
    let old = pdf("<< /FT /Tx /T (Unplaced) /V (original) >>");
    let new = pdf("<< /FT /Tx /T (Unplaced) /V (changed) >>");
    let diff = compare(&old, &new);
    assert!(diff.has_changes());
    assert_eq!(diff.summary.canonical_form_fields_changed, 1);
    assert_eq!(diff.summary.form_fields_changed, 0);
    assert_eq!(diff.summary.pages_visual_changed, 0);
    assert!(diff.pages[0].form_changes.is_empty());
    assert_eq!(
        diff.canonical_form_changes[0]
            .old
            .as_ref()
            .unwrap()
            .properties["V"],
        "original"
    );
    assert_eq!(
        diff.canonical_form_changes[0]
            .new
            .as_ref()
            .unwrap()
            .properties["V"],
        "changed"
    );
    assert!(diff
        .comparison
        .as_ref()
        .unwrap()
        .canonical_form_review_reasons
        .is_empty());
    assert!(!diff.is_comparison_complete()); // Other interactive semantics still need review.
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
        serde_json::to_value(files.canonical_form_changes).unwrap(),
        serde_json::to_value(diff.canonical_form_changes).unwrap()
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
        .arg(old_file.path())
        .arg(new_file.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["summary"]["canonical_form_fields_changed"], 1);
}

#[test]
fn every_saved_export_value_is_compared_even_when_first_selection_is_equal() {
    let old=pdf("<< /FT /Ch /T (Choices) /Ff 2097152 /Opt [(one) (two) (three)] /I [0 1] /V [(one) (two)] >>");
    let new=pdf("<< /FT /Ch /T (Choices) /Ff 2097152 /Opt [(one) (two) (three)] /I [0 2] /V [(one) (three)] >>");
    let diff = compare(&old, &new);
    assert_eq!(diff.summary.canonical_form_fields_changed, 1);
    let change = &diff.canonical_form_changes[0];
    assert_eq!(
        change.old.as_ref().unwrap().properties["V"],
        serde_json::json!(["one", "two"])
    );
    assert_eq!(
        change.new.as_ref().unwrap().properties["V"],
        serde_json::json!(["one", "three"])
    );
}

#[test]
fn equivalent_text_encodings_and_dictionary_order_do_not_change_fields() {
    let old = pdf("<< /FT /Tx /T (Name) /V (Alpha) >>");
    let new = pdf("<< /V <FEFF0041006C007000680061> /T <EFBBBF4E616D65> /FT /Tx /Ff 0 /Q 0 >>");
    let diff = compare(&old, &new);
    assert!(diff.canonical_form_changes.is_empty());
    assert_eq!(diff.comparison.unwrap().canonical_fields_examined, 2);
}

#[test]
fn malformed_text_is_reviewed_instead_of_appearing_as_field_deletion() {
    let old = pdf("<< /FT /Tx /T (Name) /V (Alpha) >>");
    let new = pdf("<< /FT /Tx /T (Name) /V <FEFF0041FF> >>");
    let diff = compare(&old, &new);
    assert!(diff.canonical_form_changes.is_empty());
    assert!(!diff
        .comparison
        .unwrap()
        .canonical_form_review_reasons
        .is_empty());
}

#[test]
fn an_unreadable_property_does_not_hide_a_known_change_on_the_same_field() {
    let old = pdf("<< /FT /Tx /T (Name) /Ff 0 /V (Alpha) >>");
    let new = pdf("<< /FT /Tx /T (Name) /Ff 1 /V <FEFF0041FF> >>");
    let diff = compare(&old, &new);
    assert!(diff.has_changes());
    assert_eq!(diff.summary.canonical_form_fields_changed, 1);
    let field = diff.canonical_form_changes[0].new.as_ref().unwrap();
    assert_eq!(field.properties["Ff"], 1);
    assert!(!field.properties.contains_key("V"));
    assert!(field.unavailable_properties["V"].contains("Odd UTF-16"));
    let coverage = diff.comparison.as_ref().unwrap();
    assert_eq!(coverage.canonical_fields_examined, 2);
    assert_eq!(coverage.canonical_properties_unavailable, 1);
    assert!(!diff.is_comparison_complete());
}

#[test]
fn text_values_and_javascript_streams_compare_by_decoded_content() {
    use lopdf::{dictionary, Document, Object, Stream};
    fn source(value: &str, script: &str, streams: bool) -> Vec<u8> {
        let mut doc = Document::load_mem(&pdf("<< /FT /Tx /T (Name) >>")).unwrap();
        let encode = |doc: &mut Document, text: &str| {
            if streams {
                let mut stream = Stream::new(dictionary! {}, text.as_bytes().to_vec());
                stream.compress().unwrap();
                Object::Reference(doc.add_object(stream))
            } else {
                lopdf::text_string(text)
            }
        };
        let value = encode(&mut doc, value);
        let script = encode(&mut doc, script);
        let field = doc.get_object_mut((6, 0)).unwrap().as_dict_mut().unwrap();
        field.set("V", value);
        field.set(
            "AA",
            dictionary! {"K"=>dictionary!{"S"=>"JavaScript","JS"=>script}},
        );
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }
    let alpha = "Alpha".repeat(128);
    let script = "event.rc=true;".repeat(128);
    let old = source(&alpha, &script, false);
    let encoded = source(&alpha, &script, true);
    let equal = compare(&old, &encoded);
    assert!(equal.canonical_form_changes.is_empty());
    assert_eq!(
        equal.comparison.unwrap().canonical_properties_unavailable,
        0
    );
    let beta = "Beta".repeat(128);
    let changed = compare(&old, &source(&beta, &"event.rc=false;".repeat(128), true));
    assert_eq!(changed.summary.canonical_form_fields_changed, 1);
    let field = changed.canonical_form_changes[0].new.as_ref().unwrap();
    assert_eq!(field.properties["V"], beta);
    assert!(field.unavailable_properties.is_empty());
    assert!(field.properties["AA"]
        .to_string()
        .contains("event.rc=false;"));
}

#[test]
fn hybrid_xfa_retains_available_acroform_value_changes() {
    use lopdf::{dictionary, Document, Object, Stream};
    fn source(value: &str) -> Vec<u8> {
        let mut doc =
            Document::load_mem(&pdf(&format!("<< /FT /Tx /T (Name) /V ({value}) >>"))).unwrap();
        let xfa=doc.add_object(Stream::new(dictionary!{},b"<xfa:datasets xmlns:xfa=\"http://www.xfa.org/schema/xfa-data/1.0/\"><xfa:data/></xfa:datasets>".to_vec()));
        doc.catalog_mut()
            .unwrap()
            .get_mut(b"AcroForm")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "XFA",
                vec![Object::string_literal("datasets"), Object::Reference(xfa)],
            );
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }
    let diff = compare(&source("Alpha"), &source("Beta"));
    assert_eq!(diff.summary.canonical_form_fields_changed, 1);
    assert_eq!(
        diff.canonical_form_changes[0]
            .new
            .as_ref()
            .unwrap()
            .properties["V"],
        "Beta"
    );
    let coverage = diff.comparison.as_ref().unwrap();
    assert_eq!(coverage.canonical_fields_examined, 2);
    assert!(coverage
        .canonical_form_review_reasons
        .iter()
        .any(|r| r.contains("XFA semantics")));
    assert!(!diff.is_comparison_complete());
}

#[test]
fn non_null_form_records_are_checked_even_when_the_sdk_reports_no_form() {
    for (case, complete, reasons) in [
        ("form_catalog_invalid", false, 2),
        ("form_catalog_null", true, 0),
        ("form_null_entries", false, 0),
    ] {
        let diff = compare(
            &appearance_fixture::fixture(case, false),
            &appearance_fixture::fixture(case, true),
        );
        assert_eq!(diff.is_comparison_complete(), complete, "{case}");
        assert_eq!(
            diff.comparison
                .as_ref()
                .unwrap()
                .canonical_form_review_reasons
                .len(),
            reasons,
            "{case}"
        );
    }
    let absent = appearance_fixture::catalog_form_entry("absent");
    for mode in ["null", "null-ref", "dangling"] {
        let other = appearance_fixture::catalog_form_entry(mode);
        let diff = compare(&absent, &other);
        assert!(
            diff.is_comparison_complete(),
            "{mode}: {:?}",
            diff.comparison
        );
        assert!(!diff.has_changes());
        assert!(diff
            .comparison
            .unwrap()
            .canonical_form_review_reasons
            .is_empty());
    }
    for mode in ["number", "name", "string", "array", "stream", "cycle"] {
        let other = appearance_fixture::catalog_form_entry(mode);
        for (a, b) in [(&absent, &other), (&other, &absent)] {
            let diff = compare(a, b);
            assert!(
                !diff.is_comparison_complete(),
                "{mode}: {:?}",
                diff.comparison
            );
            assert!(diff.is_text_comparison_complete());
            assert!(!diff
                .comparison
                .as_ref()
                .unwrap()
                .canonical_form_review_reasons
                .is_empty());
            assert!(diff.canonical_form_changes.is_empty());
            assert_eq!(diff.summary.pages_visual_changed, 0);
        }
        assert!(
            compare(&other, &other).is_comparison_complete(),
            "{mode} identity"
        );
    }
    for (mode, exit) in [("number", 3), ("null", 0)] {
        let a = tempfile::NamedTempFile::new().unwrap();
        let b = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(a.path(), &absent).unwrap();
        std::fs::write(b.path(), appearance_fixture::catalog_form_entry(mode)).unwrap();
        let diff = pdf_diff::diff_files(
            a.path(),
            b.path(),
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert_eq!(diff.is_comparison_complete(), exit == 0);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
            .arg(a.path())
            .arg(b.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{mode}: stderr={} stdout={}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn null_dictionary_entries_are_absent_but_null_array_items_are_preserved() {
    for (old, new) in [
        (
            "<< /FT /Tx /T (Name) >>",
            "<< /FT /Tx /T (Name) /V null /DV 99 0 R /Ff null /Q null /AA null >>",
        ),
        ("<< /FT /Tx >>", "<< /FT /Tx /T null >>"),
        (
            "<< /FT /Tx /T (Name) /AA << /K << /S /JavaScript /JS (var x=1;) >> >> >>",
            "<< /FT /Tx /T (Name) /AA << /K << /S /JavaScript /JS (var x=1;) /Next null >> >> >>",
        ),
    ] {
        let diff = compare(&pdf(old), &pdf(new));
        assert!(!diff.has_changes(), "{:?}", diff.canonical_form_changes);
        assert!(diff
            .comparison
            .as_ref()
            .unwrap()
            .canonical_form_review_reasons
            .is_empty());
        assert_eq!(diff.summary.pages_visual_changed, 0);
    }
    use lopdf::{dictionary, Document, Object, Stream};
    let source = pdf("<< /FT /Tx /T (Name) /V (Alpha) >>");
    let mut doc = Document::load_mem(&source).unwrap();
    let stream = doc.add_object(Stream::new(dictionary! { "Filter" => Object::Null, "DecodeParms" => Object::Null, "Unused" => Object::Null }, b"Alpha".to_vec()));
    doc.get_object_mut((6, 0))
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("V", stream);
    let mut rewritten = Vec::new();
    doc.save_to(&mut rewritten).unwrap();
    let equal = compare(&source, &rewritten);
    assert!(!equal.has_changes(), "{:?}", equal.canonical_form_changes);
    assert!(equal
        .comparison
        .as_ref()
        .unwrap()
        .canonical_form_review_reasons
        .is_empty());
    assert_eq!(
        compare(
            &pdf("<< /FT /Tx /T (Name) >>"),
            &pdf("<< /FT /Tx /T (Name) /V (null) >>")
        )
        .summary
        .canonical_form_fields_changed,
        1
    );
    let diff = compare(
        &pdf("<< /FT /Ch /T (Name) /V [null (x)] >>"),
        &pdf("<< /FT /Ch /T (Name) /V [(x)] >>"),
    );
    assert_eq!(diff.summary.canonical_form_fields_changed, 1);
    assert_eq!(
        diff.canonical_form_changes[0]
            .old
            .as_ref()
            .unwrap()
            .properties["V"],
        serde_json::json!([null, "x"])
    );
}

#[test]
fn repeated_widget_properties_do_not_erase_other_logical_field_changes() {
    for mode in [
        "same",
        "indirect",
        "flags",
        "default-flags",
        "value",
        "text-equivalent",
        "null",
        "root-unnamed",
        "child-named",
    ] {
        let old = appearance_fixture::repeated_field_control(mode, false);
        let new = appearance_fixture::repeated_field_control(mode, true);
        let diff = compare(&old, &new);
        assert_eq!(
            diff.summary.canonical_form_fields_changed, 1,
            "{mode}: {:?}",
            diff.comparison
        );
        assert_eq!(
            diff.canonical_form_changes[0].old.as_ref().unwrap().name,
            ["Other"]
        );
        assert_eq!(
            diff.canonical_form_changes[0]
                .old
                .as_ref()
                .unwrap()
                .properties["V"],
            "original"
        );
        assert_eq!(
            diff.canonical_form_changes[0]
                .new
                .as_ref()
                .unwrap()
                .properties["V"],
            "changed"
        );
        assert_eq!(diff.summary.pages_visual_changed, 0);
        assert_eq!(diff.summary.form_fields_changed, 0);
        let coverage = diff.comparison.as_ref().unwrap();
        assert_eq!(
            coverage.canonical_fields_examined,
            if mode == "child-named" { 6 } else { 4 }
        );
        assert!(
            coverage.canonical_form_review_reasons.is_empty(),
            "{mode}: {coverage:?}"
        );
        assert!(!diff.is_comparison_complete());
        assert!(diff.pages.iter().all(|p| p.form_changes.is_empty()));
    }
    let old = tempfile::NamedTempFile::new().unwrap();
    let new = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        old.path(),
        appearance_fixture::repeated_field_control("same", false),
    )
    .unwrap();
    std::fs::write(
        new.path(),
        appearance_fixture::repeated_field_control("same", true),
    )
    .unwrap();
    let files = pdf_diff::diff_files(
        old.path(),
        new.path(),
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(files.summary.canonical_form_fields_changed, 1);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
        .arg(old.path())
        .arg(new.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["summary"]["canonical_form_fields_changed"], 1);
}

#[test]
fn conflicting_widget_entries_and_genuine_duplicate_names_remain_review() {
    for mode in [
        "conflict-type",
        "conflict-flags",
        "conflict-value",
        "conflict-options",
        "conflict-action",
    ] {
        let diff = compare(
            &appearance_fixture::repeated_field_control(mode, false),
            &appearance_fixture::repeated_field_control(mode, true),
        );
        assert!(!diff.is_comparison_complete(), "{mode}");
        assert!(
            !diff
                .comparison
                .as_ref()
                .unwrap()
                .canonical_form_review_reasons
                .is_empty(),
            "{mode}"
        );
        assert!(diff.canonical_form_changes.is_empty());
    }
    use lopdf::{dictionary, Document, Object};
    let mut d = Document::load_mem(&pdf("<< /FT /Tx /T (Name) /V (one) >>")).unwrap();
    let other = d.add_object(dictionary! { "FT" => "Tx", "T" => lopdf::text_string("Name"), "V" => lopdf::text_string("two") });
    d.catalog_mut()
        .unwrap()
        .get_mut(b"AcroForm")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Fields",
            vec![Object::Reference((6, 0)), Object::Reference(other)],
        );
    let mut bytes = Vec::new();
    d.save_to(&mut bytes).unwrap();
    let mut rewritten = bytes.clone();
    rewritten.extend_from_slice(
        b"
% rewrite
",
    );
    let diff = compare(&bytes, &rewritten);
    assert!(!diff.is_comparison_complete());
    assert!(diff
        .comparison
        .as_ref()
        .unwrap()
        .canonical_form_review_reasons
        .iter()
        .any(|r| r.contains("Duplicate")));
}

#[test]
fn orphan_page_values_and_parent_inheritance_remain_comparable_without_catalog_links() {
    for mode in [
        "no-form",
        "null-form",
        "empty",
        "missing-fields",
        "parent",
        "missing-backlink",
        "alias",
    ] {
        let old = appearance_fixture::orphan_widgets(mode, false);
        let new = appearance_fixture::orphan_widgets(mode, true);
        let diff = compare(&old, &new);
        assert_eq!(
            diff.summary.canonical_form_fields_changed, 1,
            "{mode}: {diff:?}"
        );
        assert_eq!(diff.summary.pages_visual_changed, 0, "{mode}");
        assert_eq!(diff.summary.form_fields_changed, 0, "{mode}");
        assert!(!diff.is_comparison_complete(), "{mode}");
        assert!(diff
            .canonical_form_changes
            .iter()
            .all(|c| c.status == pdf_diff::Status::Modified));
        let changed = &diff.canonical_form_changes[0];
        assert_eq!(changed.old.as_ref().unwrap().properties["V"], "original");
        assert_eq!(changed.new.as_ref().unwrap().properties["V"], "changed");
        assert_eq!(
            diff.comparison.as_ref().unwrap().canonical_fields_examined,
            2
        );
        assert!(!diff
            .comparison
            .as_ref()
            .unwrap()
            .canonical_form_review_reasons
            .is_empty());
        assert!(diff.pages.iter().all(|p| p.form_changes.is_empty()));
        let directory = tempfile::tempdir().unwrap();
        let a = directory.path().join("old.pdf");
        let b = directory.path().join("new.pdf");
        std::fs::write(&a, &old).unwrap();
        std::fs::write(&b, &new).unwrap();
        let file =
            pdf_diff::diff_files(&a, &b, &Limits::default(), &DiffOptions::default()).unwrap();
        assert_eq!(file.summary.canonical_form_fields_changed, 1, "{mode}");
        let process = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
            .arg(a)
            .arg(b)
            .output()
            .unwrap();
        assert_eq!(process.status.code(), Some(3), "{mode}");
        let json: serde_json::Value = serde_json::from_slice(&process.stdout).unwrap();
        assert_eq!(
            json["summary"]["canonical_form_fields_changed"], 1,
            "{mode}"
        );
        assert!(compare(&old, &old).is_comparison_complete());
    }
    for case in ["form_orphan_merged", "form_orphan_parent"] {
        assert_eq!(
            compare(
                &appearance_fixture::fixture(case, false),
                &appearance_fixture::fixture(case, true)
            )
            .summary
            .canonical_form_fields_changed,
            1
        );
    }
}

#[test]
fn partial_orphan_inventories_do_not_invent_added_or_removed_fields() {
    let old = appearance_fixture::fixture("form_orphan_partial", false);
    let new = appearance_fixture::fixture("form_orphan_partial", true);
    for (a, b) in [(&old, &new), (&new, &old)] {
        let diff = compare(a, b);
        assert_eq!(diff.summary.canonical_form_fields_changed, 1, "{diff:?}");
        let change = &diff.canonical_form_changes[0];
        assert_eq!(change.status, pdf_diff::Status::Modified);
        assert_eq!(change.old.as_ref().unwrap().name, ["Declared"]);
        assert_eq!(change.new.as_ref().unwrap().name, ["Declared"]);
        assert_eq!(
            diff.comparison.as_ref().unwrap().canonical_fields_examined,
            4
        );
        assert!(!diff.is_comparison_complete());
    }
}

#[test]
fn cyclic_or_ambiguous_orphan_fields_remain_unknown_and_unused_pages_are_ignored() {
    for mode in ["cycle", "duplicate"] {
        let diff = compare(
            &appearance_fixture::orphan_widgets(mode, false),
            &appearance_fixture::orphan_widgets(mode, true),
        );
        assert_eq!(diff.summary.canonical_form_fields_changed, 0);
        assert_eq!(
            diff.comparison.as_ref().unwrap().canonical_fields_examined,
            0
        );
        assert!(!diff
            .comparison
            .as_ref()
            .unwrap()
            .canonical_form_review_reasons
            .is_empty());
        assert!(!diff.is_comparison_complete());
    }
    let diff = compare(
        &appearance_fixture::fixture("form_orphan_cycle", false),
        &appearance_fixture::fixture("form_orphan_cycle", true),
    );
    assert!(!diff.is_comparison_complete());
    let unused = compare(
        &appearance_fixture::orphan_widgets("unused-page", false),
        &appearance_fixture::orphan_widgets("unused-page", true),
    );
    assert_eq!(unused.summary.canonical_form_fields_changed, 0);
    assert!(unused.is_comparison_complete());
}
