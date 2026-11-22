#[path = "support/appearance_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_from_bytes, text_layer_from_bytes,
    DiffOptions, Limits, SegmentKind,
};

/// 字間を文字の空白へ置換せず、入力順・単位・表示回転で文字セルとの対応を保持する。
#[test]
fn japanese_tracking_keeps_the_original_glyph_sequence_without_false_word_spaces() {
    for mode in ["split", "single"] {
        for unit in [1. / 64., 1., 3., 64.] {
            for rotation in [0, 90, 180, 270] {
                for reverse in [false, true] {
                    let bytes =
                        fixture::japanese_tracking(mode, unit, rotation, reverse, 0.17, false);
                    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
                    assert_eq!(
                        ir.pages[0].text(),
                        "申請書の",
                        "{mode},unit={unit},rotation={rotation},reverse={reverse}"
                    );
                    let layer = text_layer_from_bytes(&bytes, 1).unwrap();
                    assert_eq!(
                        layer.iter().map(|r| r.text.as_str()).collect::<String>(),
                        "申請書の"
                    );
                    assert_eq!(layer.iter().map(|r| r.chars.len()).sum::<usize>(), 4);
                }
            }
        }
    }
}

/// 書かれた空白・欧文・ハングルの語間と、四分アキを超えた実間隔は保持する。
#[test]
fn coded_spaces_other_scripts_and_actual_word_gaps_are_preserved() {
    for reverse in [false, true] {
        for (mode, gap, expected) in [
            ("coded", 0.17, "申 請 書 の"),
            ("latin", 0.17, "A B C D"),
            ("hangul", 0.17, "한 글 신 청"),
            ("split", 0.35, "申 請 書 の"),
        ] {
            let bytes = fixture::japanese_tracking(mode, 1., 0, reverse, gap, false);
            let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
            assert_eq!(ir.pages[0].text(), expected, "{mode},reverse={reverse}");
        }
    }
}

/// ActualTextに書かれた論理空白は、見た目の字間やSDK生成空白へ読み替えない。
#[test]
fn actual_text_words_remain_owned_and_keep_their_declared_spaces() {
    for reverse in [false, true] {
        let bytes = fixture::japanese_tracking("actual", 1., 0, reverse, 0.17, false);
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        assert_eq!(ir.pages[0].text(), "申 請 書 の");
        let layer = text_layer_from_bytes(&bytes, 1).unwrap();
        assert_eq!(
            layer.iter().map(|r| r.text.as_str()).collect::<String>(),
            "申 請 書 の"
        );
        assert!(layer
            .iter()
            .all(|r| r.source_text_region && !r.source_regions.is_empty()));
    }
}

/// 補助面の和字でも、UTF-16のコード単位やUTF-8バイト位置を文字セルの添字と混同しない。
#[test]
fn supplementary_ideographs_keep_one_public_span_per_scalar() {
    for unit in [1. / 64., 1., 3., 64.] {
        let bytes = fixture::japanese_tracking("supplementary", unit, 0, false, 0.17, false);
        let expected = "\u{20000}\u{20001}\u{33479}の";
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        assert_eq!(ir.pages[0].text(), expected);
        let layer = text_layer_from_bytes(&bytes, 1).unwrap();
        assert_eq!(
            layer.iter().map(|r| r.text.as_str()).collect::<String>(),
            expected
        );
        assert_eq!(layer.iter().map(|r| r.chars.len()).sum::<usize>(), 4);
    }
}

/// 濁点の正準分解だけで字間の判定や比較結果を変えず、各スカラーのセルを保持する。
#[test]
fn decomposed_voicing_marks_keep_the_same_tracking_decision() {
    let nfd = fixture::japanese_tracking("nfd", 1., 0, false, 0.17, false);
    let nfc = fixture::japanese_tracking("nfc", 1., 0, false, 0.17, false);
    let ir = load_document_from_bytes(&nfd, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].text(), "が請書の");
    let layer = text_layer_from_bytes(&nfd, 1).unwrap();
    assert_eq!(layer.iter().map(|r| r.chars.len()).sum::<usize>(), 5);
    let diff = diff_bytes(
        "nfd",
        "nfc",
        &nfd,
        &nfc,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(diff.is_comparison_complete() && !diff.has_changes());
}

/// 同じ表示を別のオブジェクトへ分けても差分を作らず、文字だけの本当の編集は検出する。
#[test]
fn object_rewrites_stay_equivalent_and_unicode_only_edits_remain_visible() {
    let old = fixture::japanese_tracking("split", 1., 0, false, 0.17, false);
    let equivalent = fixture::japanese_tracking("single", 1., 0, false, 0.17, false);
    let new = fixture::japanese_tracking("split", 1., 0, true, 0.17, true);
    let same = diff_bytes(
        "old",
        "same",
        &old,
        &equivalent,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(same.is_comparison_complete() && !same.has_changes());
    assert_eq!(
        render_page_from_bytes(&old, 1, 400).unwrap().png,
        render_page_from_bytes(&new, 1, 400).unwrap().png
    );
    let diff = diff_bytes(
        "old",
        "new",
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
    for (kind, expected) in [(SegmentKind::Delete, "書"), (SegmentKind::Insert, "類")] {
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
