#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_from_bytes, text_layer_from_bytes,
    DiffOptions, Limits,
};

/// 埋め込み字形の形・位置を保ち、宣言した補助文字と実際の文字編集を返す。
#[test]
fn named_supplementary_glyphs_keep_declared_text_and_detect_unicode_only_edits() {
    for unit in [1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for shear in [false, true] {
                let bytes =
                    fixture::named_glyph_cells(&["A", "u1040C", "C"], unit, rotation, shear);
                let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(ir.pages[0].text(), "A𐐌C", "{unit} {rotation} {shear}");
                assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
                assert_eq!(ir.pages[0].sdk_unicode_mapping_errors, 0);
                let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                assert_eq!(
                    layer.iter().map(|r| r.text.as_str()).collect::<String>(),
                    "A𐐌C"
                );
                assert_eq!(layer.iter().map(|r| r.chars.len()).sum::<usize>(), 3);
                let mut rewrite = bytes.clone();
                rewrite.extend_from_slice(b"\n% equivalent\n");
                let equal = diff_bytes(
                    "a",
                    "b",
                    &bytes,
                    &rewrite,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert!(equal.is_comparison_complete(), "{:?}", equal.diagnostics);
                assert!(!equal.has_changes());
                let original = fixture::named_glyph_cells(&["A", "B", "C"], unit, rotation, shear);
                let edited = diff_bytes(
                    "a",
                    "b",
                    &original,
                    &bytes,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert!(edited.is_comparison_complete());
                assert_eq!(edited.summary.blocks_changed, 1);
                assert_eq!(edited.summary.pages_visual_changed, 0);
                assert_eq!(
                    render_page_from_bytes(&original, 1, 400).unwrap().png,
                    render_page_from_bytes(&bytes, 1, 400).unwrap().png
                );
            }
        }
    }
}

/// 正常なSDK文字は保持し、下位値や双方向の衝突を推測で解決しない。
#[test]
fn ambiguous_or_direction_changing_names_never_hide_an_incorrect_sdk_result() {
    for (mode, expected) in [
        ("collision", "𐐌\u{2040c}"),
        ("bmp_collision", "𐐌Ќ"),
        ("direction", "😀"),
    ] {
        let old = fixture::fixture(&format!("named_unicode_{mode}"), false);
        let new = fixture::fixture(&format!("named_unicode_{mode}"), true);
        let ir = load_document_from_bytes(&new, &Limits::default()).unwrap();
        let complete = ir.pages[0].text() == expected;
        let diff = diff_bytes(
            "a",
            "b",
            &old,
            &new,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert_eq!(
            diff.is_comparison_complete(),
            complete,
            "{mode}: {:?}",
            diff.diagnostics
        );
        assert!(!diff.has_changes());
        assert!(!ir.pages[0].source_unicode_accounting_unavailable);
        if !complete {
            assert!(ir.pages[0].source_unicode_scalars_unmatched > 0);
            let layer = text_layer_from_bytes(&new, 1).unwrap();
            assert!(layer.iter().all(|r| r.source_unicode_scalars_unmatched > 0));
        }
    }
}
