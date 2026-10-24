#[allow(dead_code)]
#[path = "support/appearance_fixture.rs"]
mod fixture;
use pdf_diff::{
    diff_bytes, load_document, load_document_from_bytes, text_layer, text_layer_from_bytes,
    DiffOptions, Limits,
};
use std::collections::BTreeMap;

fn bag(text: &str) -> BTreeMap<char, usize> {
    let mut out = BTreeMap::new();
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        *out.entry(c).or_default() += 1;
    }
    out
}
fn without_rotation(doc: &pdf_diff::DocumentIr) -> serde_json::Value {
    let mut value = serde_json::to_value(doc).unwrap();
    for p in value["pages"].as_array_mut().unwrap() {
        p.as_object_mut().unwrap().remove("rotation");
    }
    value
}

#[test]
fn display_rotation_preserves_oblique_text_and_every_character_span() {
    for unit in [1.0, 64.0] {
        for nested in [false, true] {
            let reference = fixture::angled_label_layout(unit, 0, nested, false);
            let ir = load_document_from_bytes(&reference, &Limits::default()).unwrap();
            let layer = text_layer_from_bytes(&reference, 1).unwrap();
            assert_eq!(
                bag(&ir.pages[0].text()),
                bag("Survey headingMARK 12 34 56EAST 90 20NORTH 45")
            );
            for rotation in [0, 90, 180, 270] {
                let bytes = fixture::angled_label_layout(unit, rotation, nested, false);
                let original = bytes.clone();
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("rotated.pdf");
                std::fs::write(&path, &bytes).unwrap();
                let actual = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(actual.pages[0].rotation, u64::from(rotation));
                assert_eq!(
                    without_rotation(&ir),
                    without_rotation(&actual),
                    "{unit}/{rotation}/{nested}"
                );
                assert_eq!(
                    serde_json::to_value(&actual).unwrap(),
                    serde_json::to_value(load_document(&path, &Limits::default()).unwrap())
                        .unwrap()
                );
                for runs in [
                    text_layer(&path, 1).unwrap(),
                    text_layer_from_bytes(&bytes, 1).unwrap(),
                ] {
                    assert_eq!(
                        serde_json::to_value(&layer).unwrap(),
                        serde_json::to_value(runs).unwrap()
                    );
                }
                let diff = diff_bytes(
                    "source",
                    "rotated",
                    &reference,
                    &bytes,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert_eq!(
                    diff.summary.pages_visual_changed,
                    usize::from(rotation != 0)
                );
                assert_eq!(
                    diff.summary.pages_geometry_changed,
                    usize::from(rotation != 0)
                );
                assert_eq!(
                    diff.summary.blocks_changed
                        + diff.summary.blocks_added
                        + diff.summary.blocks_removed,
                    0
                );
                assert_eq!(
                    diff.summary.blocks_moved
                        + diff.summary.blocks_resized
                        + diff.summary.blocks_format_changed,
                    0
                );
                assert_eq!(bytes, original);
                assert_eq!(std::fs::read(path).unwrap(), original);
            }
        }
    }
}

#[test]
fn actual_oblique_label_edits_still_change_text_and_pixels() {
    for rotation in [0, 90, 180, 270] {
        let old = fixture::angled_label_layout(1.0, rotation, true, false);
        let new = fixture::angled_label_layout(1.0, rotation, true, true);
        let a = load_document_from_bytes(&old, &Limits::default()).unwrap();
        let b = load_document_from_bytes(&new, &Limits::default()).unwrap();
        assert_ne!(bag(&a.pages[0].text()), bag(&b.pages[0].text()));
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
            diff.summary.blocks_changed + diff.summary.blocks_added + diff.summary.blocks_removed
                > 0
        );
        assert_eq!(diff.summary.pages_visual_changed, 1);
        assert_eq!(diff.summary.pages_geometry_changed, 0);
    }
}
