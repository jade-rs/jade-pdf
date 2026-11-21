#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_from_bytes, text_layer_from_bytes,
    DiffOptions, Limits, SegmentKind,
};

/// 文字セルが連続している別オブジェクトの字面余白を、単語区切りへ変換しない。
#[test]
fn touching_advance_cells_do_not_introduce_an_intra_word_space() {
    for unit in [1. / 64., 1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for reverse in [false, true] {
                let pdf = fixture::advance_word_layout(unit, rotation, reverse, 0., true, false);
                let ir = load_document_from_bytes(&pdf, &Limits::default()).unwrap();
                assert_eq!(ir.pages[0].text(), "18", "reverse={reverse}");
                let layer = text_layer_from_bytes(&pdf, 1).unwrap();
                assert_eq!(
                    layer
                        .iter()
                        .map(|r| r.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    "18"
                );
                let joined = fixture::advance_word_layout(unit, rotation, false, 0., false, false);
                let diff = diff_bytes(
                    "joined",
                    "split",
                    &joined,
                    &pdf,
                    &Limits::default(),
                    &DiffOptions::default(),
                )
                .unwrap();
                assert!(
                    diff.is_comparison_complete() && !diff.has_changes(),
                    "unit={unit},rotation={rotation},reverse={reverse}"
                );
                if unit == 1. && rotation == 0 {
                    assert_eq!(
                        render_page_from_bytes(&joined, 1, 400).unwrap().png,
                        render_page_from_bytes(&pdf, 1, 400).unwrap().png
                    );
                }
            }
        }
    }
}

/// 本物の文字送り間の空きは、字面余白の修正で単語へ連結しない。
#[test]
fn real_word_gaps_are_preserved() {
    for reverse in [false, true] {
        let pdf = fixture::advance_word_layout(1., 0, reverse, 6., true, false);
        let ir = load_document_from_bytes(&pdf, &Limits::default()).unwrap();
        assert_eq!(ir.pages[0].text(), "1 8", "reverse={reverse}");
    }
}

/// 字面が同じでも、本当の一文字編集を表示差分へ依存せず検出する。
#[test]
fn unicode_only_edit_remains_visible_to_the_diff() {
    let old = fixture::advance_word_layout(1., 0, false, 0., true, false);
    let new = fixture::advance_word_layout(1., 0, true, 0., true, true);
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
    assert_eq!(diff.summary.pages_visual_changed, 0);
    let segments = &diff.pages[0].blocks[0].segments;
    assert_eq!(
        segments
            .iter()
            .filter(|s| matches!(s.kind, SegmentKind::Delete))
            .map(|s| s.text.as_str())
            .collect::<String>(),
        "8"
    );
    assert_eq!(
        segments
            .iter()
            .filter(|s| matches!(s.kind, SegmentKind::Insert))
            .map(|s| s.text.as_str())
            .collect::<String>(),
        "7"
    );
}
