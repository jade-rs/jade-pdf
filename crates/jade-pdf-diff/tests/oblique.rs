#[allow(dead_code)]
#[path = "support/appearance_fixture.rs"]
mod fixture;
use pdf_diff::{load_document_from_bytes, text_layer_from_bytes, Limits};

fn words(bytes: &[u8]) -> Vec<String> {
    let doc = load_document_from_bytes(bytes, &Limits::default()).unwrap();
    let layer = text_layer_from_bytes(bytes, 1).unwrap();
    assert_eq!(
        doc.pages[0]
            .blocks
            .iter()
            .map(|b| &b.text)
            .collect::<Vec<_>>(),
        layer.iter().map(|b| &b.text).collect::<Vec<_>>()
    );
    assert!(layer
        .iter()
        .all(|r| r.text.chars().count() == r.chars.len()));
    doc.pages[0].blocks.iter().map(|b| b.text.clone()).collect()
}

#[test]
fn oblique_source_words_survive_rotations_shear_and_anisotropic_scales() {
    for degrees in [15.0, 45.0, 75.0, 135.0, 225.0, 315.0] {
        for (unit, rotation, shear, xs, ys, mode) in [
            (1.0, 0, 0.0, 1.0, 1.0, "whole"),
            (64.0, 90, 0.0, 1.0, 1.0, "letters"),
            (1.0, 180, 0.25, 1.5, 0.75, "parallel"),
            (64.0, 270, -0.25, 0.75, 1.5, "nested"),
            (1.0, 90, 0.0, -1.0, 1.0, "whole"),
            (1.0, 270, 0.0, 1.0, 1.0, "negative-size"),
        ] {
            let source =
                fixture::oblique_layout(unit, rotation, degrees, [shear, xs, ys], mode, false);
            let got = words(&source);
            assert!(
                got.iter().any(|s| s == "ALPHA 12 BETA"),
                "{degrees}/{unit}/{rotation}/{mode}: {got:?}"
            );
            if mode == "parallel" {
                assert_eq!(got.len(), 2);
                assert!(got.iter().any(|s| s == "OTHER 77"));
            } else {
                assert_eq!(got.len(), 1);
            }
        }
    }
}

#[test]
fn oblique_labels_do_not_join_across_baselines_backtracking_or_wide_gutters() {
    for mode in ["crossed", "backtrack", "far"] {
        for degrees in [45.0, 135.0, 225.0, 315.0] {
            let source = fixture::oblique_layout(64.0, 90, degrees, [0.2, 1.5, 0.75], mode, false);
            let got = words(&source);
            assert_eq!(got.len(), 2, "{mode}/{degrees}: {got:?}");
            if mode == "far" {
                assert!(got.iter().any(|s| s == "ALPHA"));
                assert!(got.iter().any(|s| s == "BETA"));
            } else {
                assert!(got.iter().any(|s| s == "A"));
                assert!(got.iter().any(|s| s == "B"));
            }
        }
    }
}
