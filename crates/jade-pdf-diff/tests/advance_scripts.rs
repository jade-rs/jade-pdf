#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_from_bytes, text_layer_from_bytes,
    DiffOptions, Limits, SegmentKind,
};

/// 接する送りセルの添字を基底と次の演算子の間に保ち、実際の文字間隔は別行として保持する。
#[test]
fn verified_script_spacing_preserves_order_across_units_rotation_and_source_order() {
    for unit in [1. / 64., 1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for reverse in [false, true] {
                for (gap, expected) in [(0., "b10="), (6., "b=10")] {
                    let bytes = fixture::advance_script_layout(unit, rotation, reverse, gap, false);
                    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                    let text = ir.pages[0].text();
                    assert_eq!(
                        text.split_whitespace().collect::<String>(),
                        expected,
                        "unit={unit},rotation={rotation},reverse={reverse},gap={gap}"
                    );
                    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                    assert_eq!(
                        layer.iter().map(|r| r.text.as_str()).collect::<String>(),
                        ir.pages[0]
                            .blocks
                            .iter()
                            .map(|r| r.text.as_str())
                            .collect::<String>()
                    );
                }
            }
        }
    }
}

/// 字面を変えない添字の実編集は、正しい行内位置で一文字の削除と挿入として検出する。
#[test]
fn unicode_only_script_edit_retains_exact_changed_scalar() {
    let old = fixture::advance_script_layout(1., 0, false, 0., false);
    let new = fixture::advance_script_layout(1., 0, true, 0., true);
    assert_eq!(
        render_page_from_bytes(&old, 1, 400).unwrap().png,
        render_page_from_bytes(&new, 1, 400).unwrap().png
    );
    let diff = diff_bytes(
        "a",
        "b",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(diff.is_comparison_complete() && diff.has_changes());
    assert_eq!(diff.summary.pages_visual_changed, 0);
    let segments = diff
        .pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .flat_map(|b| b.segments.iter())
        .collect::<Vec<_>>();
    for (kind, expected) in [(SegmentKind::Delete, "0"), (SegmentKind::Insert, "2")] {
        assert_eq!(
            segments
                .iter()
                .filter(|s| s.kind == kind)
                .map(|s| s.text.as_str())
                .collect::<String>(),
            expected
        );
    }
}
