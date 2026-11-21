#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_from_bytes, text_layer_from_bytes,
    DiffOptions, Limits,
};

/// MT Extraの省略記号とスクリプト小文字lを、SymbolのKappa・lambdaへ混同しない。
#[test]
fn mt_extra_uses_its_own_verified_encoding() {
    let bytes = fixture::mt_extra_glyph_cells("valid", 1., 0, false);
    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text(), "…ℓ");
    assert!(!ir.pages[0].source_unicode_accounting_unavailable);
    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
    assert_eq!(
        layer.iter().map(|r| r.text.as_str()).collect::<String>(),
        "…ℓ"
    );
    assert_eq!(layer.iter().map(|r| r.chars.len()).sum::<usize>(), 2);
}

/// 字体の不一致や括弧の組み立て片を推測でUnicode文字へ変換しない。
#[test]
fn mt_extra_unverified_glyphs_remain_reviewable() {
    for mode in ["wrong_program", "fragment"] {
        let bytes = fixture::mt_extra_glyph_cells(mode, 1., 0, false);
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        assert!(ir.pages[0].source_unicode_accounting_unavailable, "{mode}");
        assert!(!ir.pages[0].text().contains('λ'));
        assert!(!ir.pages[0].text().contains('Κ'));
    }
}

/// 単位・表示回転・せん断に依存せず、元のセルと実際の文字編集を保持する。
#[test]
fn mt_extra_geometry_and_edit_detection_are_covariant() {
    for unit in [1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for shear in [false, true] {
                let bytes = fixture::mt_extra_glyph_cells("valid", unit, rotation, shear);
                let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert_eq!(ir.pages[0].text(), "…ℓ");
                assert!(!ir.pages[0].source_unicode_accounting_unavailable);
                let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                assert_eq!(
                    layer.iter().map(|r| r.text.as_str()).collect::<String>(),
                    "…ℓ"
                );
                let mut rewritten = bytes.clone();
                rewritten.extend_from_slice(b"\n% equivalent\n");
                let same = diff_bytes(
                    "a",
                    "b",
                    &bytes,
                    &rewritten,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert!(same.is_comparison_complete() && !same.has_changes());
                let edited = fixture::mt_extra_glyph_cells("edit", unit, rotation, shear);
                let change = diff_bytes(
                    "a",
                    "b",
                    &bytes,
                    &edited,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert!(change.is_comparison_complete());
                assert_eq!(change.summary.blocks_changed, 1);
                assert_eq!(change.summary.pages_visual_changed, 0);
                assert_eq!(
                    render_page_from_bytes(&bytes, 1, 400).unwrap().png,
                    render_page_from_bytes(&edited, 1, 400).unwrap().png
                );
            }
        }
    }
    let outside = fixture::mt_extra_glyph_cells("outside_actual", 1., 0, false);
    let ir = load_document_from_bytes(&outside, &Limits::default()).unwrap();
    assert!(ir.pages[0].text().contains("…ℓ") && ir.pages[0].text().contains('A'));
    let actual = fixture::mt_extra_glyph_cells("actual", 1., 0, false);
    let ir = load_document_from_bytes(&actual, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text(), "\u{f04b}\u{f06c}");
    let collision = fixture::mt_extra_glyph_cells("actual_collision", 1., 0, false);
    let ir = load_document_from_bytes(&collision, &Limits::default()).unwrap();
    assert!(ir.pages[0].source_unicode_accounting_unavailable);
    assert!(!ir.pages[0].text().contains('ℓ'));
}
