#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_from_bytes, text_layer_from_bytes,
    DiffOptions, Limits,
};

/// 私用領域のSymbol符号を、字形IDと埋め込みフォントの対応から復元する。
#[test]
fn verified_symbol_cmap_recovers_visible_operators_without_changing_cells() {
    let bytes = fixture::symbol_glyph_cells("valid", 1., 0, false);
    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text(), "≤<σ");
    assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
    assert_eq!(ir.pages[0].sdk_unicode_mapping_errors, 0);
    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
    assert_eq!(
        layer.iter().map(|r| r.text.as_str()).collect::<String>(),
        "≤<σ"
    );
    assert_eq!(layer.iter().map(|r| r.chars.len()).sum::<usize>(), 3);
}

/// 別のActualText領域を保持し、外側の検証済みセルだけを復元する。
#[test]
fn unrelated_actual_text_does_not_hide_verified_symbol_glyphs() {
    let bytes = fixture::symbol_glyph_cells("outside_actual", 1., 0, false);
    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    let text = ir.pages[0].text();
    assert!(text.contains("≤<σ"), "{text:?}");
    assert!(text.contains('A'));
    assert!(ir.pages[0].reading_order_requires_review);
    assert!(!ir.pages[0].source_unicode_accounting_unavailable);
    let collision = fixture::symbol_glyph_cells("actual_collision", 1., 0, false);
    let ir = load_document_from_bytes(&collision, &Limits::default()).unwrap();
    assert!(!ir.pages[0].text().contains('≤'));
    assert!(ir.pages[0].source_unicode_accounting_unavailable);
}

/// 同じ字形の意味の編集を検出し、等価書き換えと表示の変換では誤差分を作らない。
#[test]
fn symbol_projection_preserves_geometry_and_detects_real_unicode_edits() {
    for unit in [1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for shear in [false, true] {
                let bytes = fixture::symbol_glyph_cells("valid", unit, rotation, shear);
                let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(ir.pages[0].text(), "≤<σ", "{unit} {rotation} {shear}");
                assert!(!ir.pages[0].source_unicode_accounting_unavailable);
                let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                assert_eq!(
                    layer.iter().map(|r| r.text.as_str()).collect::<String>(),
                    "≤<σ"
                );
                let mut equal = bytes.clone();
                equal.extend_from_slice(b"\n% equivalent\n");
                let diff = diff_bytes(
                    "a",
                    "b",
                    &bytes,
                    &equal,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert!(diff.is_comparison_complete());
                assert!(!diff.has_changes());
                let edit = fixture::symbol_glyph_cells("edit", unit, rotation, shear);
                let diff = diff_bytes(
                    "a",
                    "b",
                    &bytes,
                    &edit,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert!(diff.is_comparison_complete());
                assert_eq!(diff.summary.blocks_changed, 1);
                assert_eq!(diff.summary.pages_visual_changed, 0);
                assert_eq!(
                    render_page_from_bytes(&bytes, 1, 400).unwrap().png,
                    render_page_from_bytes(&edit, 1, 400).unwrap().png
                );
            }
        }
    }
}

/// フォント名だけの宣言、異なる字形ID、曖昧なDelta・muを推測で復元しない。
#[test]
fn unverified_symbol_profiles_and_actual_text_are_preserved() {
    for mode in [
        "wrong_family",
        "wrong_program",
        "unicode_cmap",
        "wrong_cid",
        "wrong_cidmap",
        "ambiguous",
        "actual",
    ] {
        let bytes = fixture::symbol_glyph_cells(mode, 1., 0, false);
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        let text = ir.pages[0].text();
        assert!(
            !text.contains('≤') && !text.contains('σ'),
            "{mode}: {text:?}"
        );
        if !matches!(mode, "wrong_family" | "actual") {
            assert!(ir.pages[0].source_unicode_accounting_unavailable, "{mode}");
        }
    }
}
