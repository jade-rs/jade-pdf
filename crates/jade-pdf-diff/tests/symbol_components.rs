#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_from_bytes, text_layer_from_bytes,
    DiffOptions, Limits, SegmentKind,
};

/// 20部品の文字を推測せず保持し、所有の一致した既知の記号だけを復元する。
#[test]
fn verified_context_preserves_components_and_recovers_known_symbols() {
    let bytes = fixture::symbol_component_cells("valid", 1., 0, false);
    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    let expected = format!(
        "≤−{}",
        fixture::symbol_component_codes()
            .iter()
            .map(|code| char::from_u32(0xf000 + u32::from(*code)).unwrap())
            .collect::<String>()
    );
    assert_eq!(ir.pages[0].text(), expected);
    assert!(ir.pages[0].source_unicode_accounting_unavailable);
    assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
    assert_eq!(ir.pages[0].sdk_unicode_mapping_errors, 0);
    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
    assert_eq!(
        layer.iter().map(|r| r.text.as_str()).collect::<String>(),
        expected
    );
    assert_eq!(layer.iter().map(|r| r.chars.len()).sum::<usize>(), 22);
}

/// 単位・表示回転・せん断で所有証明を保持し、要確認を抽出成功へ読み替えない。
#[test]
fn component_context_is_geometry_covariant_and_equivalent_rewrites_stay_unchanged() {
    for unit in [1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for shear in [false, true] {
                let bytes = fixture::symbol_component_cells("valid", unit, rotation, shear);
                let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                assert!(
                    ir.pages[0].text().starts_with("≤−"),
                    "unit={unit},rotation={rotation},shear={shear}"
                );
                assert!(ir.pages[0].source_unicode_accounting_unavailable);
                let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                assert_eq!(
                    layer.iter().map(|r| r.text.as_str()).collect::<String>(),
                    ir.pages[0].text()
                );
                let mut rewrite = bytes.clone();
                rewrite.extend_from_slice(b"\n% equivalent\n");
                let diff = diff_bytes(
                    "a",
                    "b",
                    &bytes,
                    &rewrite,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert!(!diff.is_comparison_complete() && !diff.has_changes());
            }
        }
    }
}

/// 未確定部品のあるページでも、確定した文字の実編集を表示差に依存せず報告する。
#[test]
fn known_unicode_edit_is_reported_while_component_review_remains() {
    let old = fixture::symbol_component_cells("valid", 1., 0, false);
    let new = fixture::symbol_component_cells("edit", 1., 0, false);
    let diff = diff_bytes(
        "a",
        "b",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!diff.is_comparison_complete() && diff.has_changes());
    assert_eq!(diff.summary.pages_visual_changed, 0);
    assert_eq!(
        render_page_from_bytes(&old, 1, 400).unwrap().png,
        render_page_from_bytes(&new, 1, 400).unwrap().png
    );
    let segments = diff
        .pages
        .iter()
        .flat_map(|p| &p.blocks)
        .flat_map(|b| &b.segments)
        .collect::<Vec<_>>();
    assert_eq!(
        segments
            .iter()
            .filter(|s| matches!(s.kind, SegmentKind::Delete))
            .map(|s| s.text.as_str())
            .collect::<String>(),
        "≤"
    );
    assert_eq!(
        segments
            .iter()
            .filter(|s| matches!(s.kind, SegmentKind::Insert))
            .map(|s| s.text.as_str())
            .collect::<String>(),
        "<"
    );
}

/// 同じ描画位置でも別の原文オブジェクトに属する二文字は、正しい使用回数を保つ。
#[test]
fn distinct_object_owners_preserve_overlapping_source_characters() {
    let bytes = fixture::symbol_component_cells("overlap", 1., 0, false);
    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text().chars().filter(|c| *c == '≤').count(), 2);
    assert_eq!(
        ir.pages[0]
            .text()
            .chars()
            .filter(|c| !c.is_whitespace())
            .count(),
        23
    );
    assert_eq!(ir.pages[0].source_unicode_scalars_unmatched, 0);
    assert!(ir.pages[0].source_unicode_accounting_unavailable);
}

/// 別字体・ActualTextの衝突、未知の私用符号、異なるCIDやセルの共有へ所有を貸さない。
#[test]
fn component_ownership_must_cover_every_occurrence_without_collision() {
    for mode in [
        "wrong_cid",
        "wrong_cidmap",
        "wrong_program",
        "unicode_cmap",
        "unknown",
        "unowned",
        "actual_collision",
        "same_cell",
    ] {
        let bytes = fixture::symbol_component_cells(mode, 1., 0, false);
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        assert!(
            !ir.pages[0].text().contains('≤') && !ir.pages[0].text().contains('−'),
            "{mode}: {:?}",
            ir.pages[0].text()
        );
        assert!(ir.pages[0].source_unicode_accounting_unavailable, "{mode}");
    }
}
