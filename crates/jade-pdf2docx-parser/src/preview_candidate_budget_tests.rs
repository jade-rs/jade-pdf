//! preview候補の復号容量・再処理累積と不正文字列からの復旧を固定する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// preview以外の制約を既定値に保ち、共有metadata容量を指定する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// ASCII単一候補の本文コピー・3回のbyte/UTF-8復号・索引/完成文字列を数える。
fn literal_cost(input: &[u8], length: usize) -> u64 {
    (input.len() + 4 * std::mem::size_of::<Option<(String, ())>>() + 8 * length) as u64
}

/// raw byteと復号UTF-8を確保前に検査し、最初の超過容量を保持する。
#[test]
fn preview_candidate_decode_capacity_is_checked_before_output() {
    let bytes = b"BT (Hello) Tj ET";
    for stage in [5, 10] {
        let actual = bytes.len() as u64 + stage;
        let target = budget(actual - 1);
        assert!(extract_text_preview_from_pdf_bytes(&target, bytes).is_empty());
        assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", actual: found, ..
        }) if found == actual));
        assert!(extract_text_preview_from_pdf_bytes(&target, bytes).is_empty());
    }
}

/// 後続passの復号失敗で先行previewを公開せず、全要求容量の上限はinclusiveにする。
#[test]
fn preview_candidate_capacity_is_inclusive_and_discards_partial_result() {
    let bytes = b"BT (Hello) Tj ET";
    let cost = literal_cost(bytes, 5);
    let enough = budget(cost);
    assert_eq!(extract_text_preview_from_pdf_bytes(&enough, bytes), "Hello");
    assert!(enough.check().is_ok());
    let short = budget(cost - 1);
    assert!(extract_text_preview_from_pdf_bytes(&short, bytes).is_empty());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost)
    );
}

/// 完成候補を解放しても再処理分を予約し、エラー取得後の停止を維持する。
#[test]
fn preview_candidate_recreation_is_cumulative() {
    let bytes = b"BT (Hello) Tj ET";
    let cost = literal_cost(bytes, 5);
    let target = budget(2 * cost - 1);
    assert_eq!(extract_text_preview_from_pdf_bytes(&target, bytes), "Hello");
    assert!(extract_text_preview_from_pdf_bytes(&target, bytes).is_empty());
    assert!(
        matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == 2 * cost)
    );
    assert!(extract_text_preview_from_pdf_bytes(&target, bytes).is_empty());
}

/// 不正UTF-8の本文全体を置換せず、literalの置換byteだけを保存する。
#[test]
fn preview_binary_literal_counts_replacement_output_only() {
    let bytes = b"BT (\xff\xff) Tj ET";
    let cost = (bytes.len() + 4 * std::mem::size_of::<Option<(String, ())>>() + 12 + 3 * 8) as u64;
    let target = budget(cost);
    assert_eq!(
        extract_text_preview_from_pdf_bytes(&target, bytes),
        "\u{fffd}\u{fffd}"
    );
    assert!(target.check().is_ok());
    let short = budget(cost - 1);
    assert!(extract_text_preview_from_pdf_bytes(&short, bytes).is_empty());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost)
    );
}

/// TJのraw/Unicodeと新結合領域の全容量を数え、候補保存配列を作らない。
#[test]
fn preview_tj_join_has_no_fragment_array() {
    let bytes = b"BT [(AB) -120 <4344> (EF)] TJ ET";
    let cost = (bytes.len() + 4 * std::mem::size_of::<Option<(String, ())>>() + 12 + 3 * 24) as u64;
    let target = budget(cost);
    assert_eq!(
        extract_text_preview_from_pdf_bytes(&target, bytes),
        "ABCDEF"
    );
    assert!(target.check().is_ok());
    let short = budget(cost - 1);
    assert!(extract_text_preview_from_pdf_bytes(&short, bytes).is_empty());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost)
    );
}

/// コメントの偽operandとoperator名の末尾一致を文字候補として復号しない。
#[test]
fn preview_ignores_comment_strings_and_operator_suffixes() {
    let target = DecodeBudget::default();
    let bytes = b"BT\n% (CommentNoise) Tj\n(WrongOperator) TjSuffix\n(Visible) Tj ET";
    assert_eq!(
        extract_text_preview_from_pdf_bytes(&target, bytes),
        "Visible"
    );
    assert!(target.check().is_ok());
}

/// 容量制限後も閉じないliteral/hex/TJから後続の正常文字列へ復旧する。
#[test]
fn preview_preserves_malformed_string_recovery() {
    for bytes in [
        b"BT (Broken Tj (Recovered) Tj ET".as_slice(),
        b"BT <4142 Tj (Recovered) Tj ET",
        b"BT [(Broken) -120 (Broken) Tj ET BT (Recovered) Tj ET",
    ] {
        let target = DecodeBudget::default();
        assert_eq!(
            extract_text_preview_from_pdf_bytes(&target, bytes),
            "Recovered"
        );
        assert!(target.check().is_ok());
    }
}

/// literal内のBT/ETやコメントで表示範囲を誤って切らない。
#[test]
fn preview_text_object_markers_inside_strings_are_data() {
    let bytes = b"BT (Before ET after BT) Tj % ET\n (Visible) Tj ET";
    assert_eq!(
        extract_text_preview_from_pdf_bytes(&DecodeBudget::default(), bytes),
        "Before ET after BT\nVisible"
    );
}

/// 復号と候補保存を介さず、状態stackの要求容量だけを観測する。
fn rendering_candidates(target: &DecodeBudget, bytes: &[u8]) -> Option<bool> {
    preview_candidates::visit(
        target,
        bytes,
        &ToUnicodeCMap::default(),
        &Default::default(),
        preview_candidates::Mode::Shown,
        |_| Some(true),
    )
}

/// 不可視operandは復号も候補保存もせず、文字容量を要求しない。
#[test]
fn invisible_preview_candidates_do_not_allocate_decoded_text() {
    let target = budget(0);
    assert_eq!(
        rendering_candidates(&target, b"3 Tr (SECRET) Tj 7 Tr [(HIDDEN)] TJ"),
        Some(true)
    );
    target.check().unwrap();
}

/// q/Qの最大同時深度だけを保存し、容量境界と再利用を維持する。
#[test]
fn preview_rendering_stack_capacity_is_inclusive_and_reused() {
    let bytes = b"q Q q Q q q q q Q Q Q Q";
    let capacity = (4 * std::mem::size_of::<crate::text_rendering::Mode>()) as u64;
    let short = budget(capacity - 1);
    assert_eq!(rendering_candidates(&short, bytes), None);
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == capacity)
    );
    let enough = budget(capacity);
    assert_eq!(rendering_candidates(&enough, bytes), Some(true));
    enough.check().unwrap();
}

/// 保存領域を解放した再走査も累積し、エラー取得後は停止を維持する。
#[test]
fn preview_rendering_stack_recreation_is_cumulative() {
    let capacity = (4 * std::mem::size_of::<crate::text_rendering::Mode>()) as u64;
    let target = budget(2 * capacity - 1);
    assert_eq!(rendering_candidates(&target, b"q Q"), Some(true));
    assert_eq!(rendering_candidates(&target, b"q Q"), None);
    assert!(
        matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == 2 * capacity)
    );
    assert_eq!(rendering_candidates(&target, b"q Q"), None);
}

/// 互換previewのq保存にも既存の描画状態深度制限を適用する。
#[test]
fn preview_rendering_stack_depth_is_bounded() {
    let target = DecodeBudget::new(&ParseLimits {
        max_container_nesting: 1,
        ..Default::default()
    });
    assert_eq!(rendering_candidates(&target, b"q q Q Q"), None);
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "Form graphics state depth",
            limit: 1,
            actual: 2
        })
    ));
    assert_eq!(rendering_candidates(&target, b"0 Tr (VISIBLE) Tj"), None);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// 任意ASCII literalの復号・保存を全要求容量で制限し、再処理にも累積する。
    #[test]
    fn preview_literal_capacity_matches_model(text in "[A-Za-z]{2,30}") {
        let bytes = format!("BT ({text}) Tj ET");
        let cost = literal_cost(bytes.as_bytes(), text.len());
        let target = budget(cost);
        prop_assert_eq!(extract_text_preview_from_pdf_bytes(&target, bytes.as_bytes()), text.as_str());
        prop_assert!(target.check().is_ok());
        let short = budget(cost - 1);
        prop_assert!(extract_text_preview_from_pdf_bytes(&short, bytes.as_bytes()).is_empty());
        prop_assert!(matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost), "全要求容量を数える");
        let repeated = budget(2 * cost - 1);
        prop_assert_eq!(extract_text_preview_from_pdf_bytes(&repeated, bytes.as_bytes()), text.as_str());
        prop_assert!(extract_text_preview_from_pdf_bytes(&repeated, bytes.as_bytes()).is_empty());
        prop_assert!(matches!(repeated.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == 2 * cost), "再処理にも累積する");
    }
}
