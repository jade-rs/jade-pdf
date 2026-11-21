#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_from_bytes, text_layer_from_bytes,
    DiffOptions, Limits, SegmentKind,
};

/// 字号格子の微小差で、同じ行の一文字編集を複数の削除・追加へ分断しない。
#[test]
fn one_operator_edit_stays_local_under_units_rotation_and_translation() {
    for unit in [1. / 64., 1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for shift in [-200., 0., 100.] {
                let old = fixture::baseline_bucket_layout(unit, rotation, shift, false);
                let new = fixture::baseline_bucket_layout(unit, rotation, shift, true);
                let doc = load_document_from_bytes(&new, &Limits::default()).unwrap();
                assert_eq!(
                    doc.pages[0].blocks.len(),
                    1,
                    "unit={unit},rotation={rotation},shift={shift}"
                );
                assert_eq!(
                    doc.pages[0].text().split_whitespace().collect::<String>(),
                    "where0≤t≤79,"
                );
                let layer = text_layer_from_bytes(&new, 1).unwrap();
                assert_eq!(layer.len(), 1);
                assert_eq!(layer[0].text, doc.pages[0].text());
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
                assert_eq!(diff.summary.blocks_added, 0);
                assert_eq!(diff.summary.blocks_removed, 0);
                let block = &diff.pages[0].blocks[0];
                let deleted: String = block
                    .segments
                    .iter()
                    .filter(|s| matches!(s.kind, SegmentKind::Delete))
                    .map(|s| s.text.as_str())
                    .collect();
                let inserted: String = block
                    .segments
                    .iter()
                    .filter(|s| matches!(s.kind, SegmentKind::Insert))
                    .map(|s| s.text.as_str())
                    .collect();
                assert_eq!(deleted, "<");
                assert_eq!(inserted, "≤");
                assert_eq!(diff.summary.pages_visual_changed, 0);
                if unit == 1. && rotation == 0 && shift == 0. {
                    assert_eq!(
                        render_page_from_bytes(&old, 1, 400).unwrap().png,
                        render_page_from_bytes(&new, 1, 400).unwrap().png
                    );
                    let mut rewrite = new.clone();
                    rewrite.extend_from_slice(b"\n% unchanged source\n");
                    let same = diff_bytes(
                        "a",
                        "b",
                        &new,
                        &rewrite,
                        &Limits::default(),
                        &DiffOptions::default(),
                    )
                    .unwrap();
                    assert!(same.is_comparison_complete() && !same.has_changes());
                }
            }
        }
    }
}
