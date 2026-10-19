//! 未対応描画の履歴と容量を、抽出準備から独立して検査する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
};

/// 空Formのobjectを用意する。
fn source() -> &'static [u8] {
    b"%PDF-1.7\n3 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length 0 >>\nstream\n\nendstream\nendobj\n%%EOF"
}

/// 走査だけに適用するmetadata予算を作る。
fn budget(capacity: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity as u64,
        ..Default::default()
    })
}

/// 抽出済み参照を別予算で用意し、走査結果と停止エラーを返す。
fn run(
    contents: &[(usize, Vec<u8>)],
    capacity: usize,
) -> (Vec<QualityIssue>, Result<(), ParseError>) {
    let preparation = budget(usize::MAX);
    let objects = crate::object_index::build(&preparation, source()).unwrap();
    let references = References::new(
        &preparation,
        (0..6)
            .flat_map(|page| [(page, "F".into(), 3), (page, "Alias".into(), 3)])
            .collect(),
    )
    .unwrap();
    let target = budget(capacity);
    let mut report = QualityReport::default();
    scan(&target, &objects, &references, contents, &mut report);
    (report.issues, target.check())
}

/// 一つのForm診断の期待値を作る。
fn form(page: usize) -> QualityIssue {
    QualityIssue::UnsupportedXObject {
        page,
        object_number: 3,
        reason: UnsupportedXObjectReason::Form,
    }
}

/// 状態・集合・診断の初回slot要求容量を計算する。
fn first_form_capacity() -> usize {
    4 * (size_of::<(usize, ContentState)>()
        + size_of::<Option<(usize, u32)>>()
        + size_of::<QualityIssue>())
}

/// inclusive境界、別名の重複、不要なoperandによる追加確保なしを固定する。
#[test]
fn exact_capacity_deduplicates_aliases_without_name_storage() {
    let contents = [(0, b"/F Do /Alias Do /F# Do /missing Do /#46 Do".to_vec())];
    let capacity = first_form_capacity();
    let (issues, result) = run(&contents, capacity);
    assert_eq!(issues, [form(0)]);
    result.unwrap();
    let (issues, result) = run(&contents, capacity - 1);
    assert!(issues.is_empty());
    assert!(
        matches!(result, Err(ParseError::ResourceLimit { actual, .. }) if actual == capacity as u64)
    );
}

/// 非昇順・交互のContentsでも、借用operandとcontainer深度を所属ページで保持する。
#[test]
fn split_operands_and_containers_keep_page_identity() {
    let contents = [
        (4, b"/F".to_vec()),
        (1, b"[ /F".to_vec()),
        (0, b"/Alias".to_vec()),
        (1, b"Do BI ] /F".to_vec()),
        (4, b"Do".to_vec()),
        (0, b"Do".to_vec()),
        (1, b"Do".to_vec()),
    ];
    let (issues, result) = run(&contents, usize::MAX);
    result.unwrap();
    assert_eq!(issues, [form(4), form(0), form(1)]);
}

/// inline image以降のbinaryと別Contentsをそのページだけで走査停止する。
#[test]
fn inline_image_stops_only_its_page() {
    let contents = [
        (1, b"BI /F Do EI".to_vec()),
        (1, b"/F Do BI".to_vec()),
        (0, b"/F Do BI /Alias Do".to_vec()),
    ];
    let (issues, result) = run(&contents, usize::MAX);
    result.unwrap();
    assert_eq!(
        issues,
        [
            QualityIssue::UnsupportedInlineImage { page: 1 },
            form(0),
            QualityIssue::UnsupportedInlineImage { page: 0 }
        ]
    );
}

/// 状態配列の拡張不足で完了した診断だけを保存する。
#[test]
fn page_state_growth_preserves_completed_prefix() {
    let capacity = 4 * (size_of::<(usize, ContentState)>() + size_of::<QualityIssue>());
    let contents = (0..6)
        .map(|page| (page, b"BI".to_vec()))
        .collect::<Vec<_>>();
    let (issues, result) = run(&contents, capacity);
    assert_eq!(
        issues,
        (0..4)
            .map(|page| QualityIssue::UnsupportedInlineImage { page })
            .collect::<Vec<_>>()
    );
    assert!(
        matches!(result, Err(ParseError::ResourceLimit { actual, .. }) if actual == (capacity + 8 * size_of::<(usize, ContentState)>()) as u64)
    );
}

/// 既存の出力容量を再利用しても、状態と集合の再作成は累積する。
#[test]
fn recreated_states_are_cumulative_and_stop_survives_check() {
    let preparation = budget(usize::MAX);
    let objects = crate::object_index::build(&preparation, source()).unwrap();
    let references = References::new(&preparation, vec![(0, "F".into(), 3)]).unwrap();
    let target = budget(first_form_capacity());
    let contents = [(0, b"/F Do".to_vec())];
    let mut report = QualityReport::default();
    scan(&target, &objects, &references, &contents, &mut report);
    target.check().unwrap();
    scan(&target, &objects, &references, &contents, &mut report);
    assert!(target.check().is_err());
    scan(
        &target,
        &objects,
        &references,
        &[(0, b"BI".to_vec())],
        &mut report,
    );
    assert_eq!(report.issues, [form(0)]);
}

/// 独立容量モデルで全新配列要求を先に加算する。
fn reserve(remaining: &mut usize, slots: usize, item: usize) -> bool {
    let bytes = slots * item;
    if bytes > *remaining {
        return false;
    }
    *remaining -= bytes;
    true
}

/// 生成した一operator/Contents列を、ページ状態と初出診断の独立モデルで処理する。
fn model(events: &[(usize, u8)], mut remaining: usize) -> (Vec<QualityIssue>, bool) {
    let mut states = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut issues = Vec::new();
    let (mut state_slots, mut seen_slots, mut output_slots) = (0, 0, 0);
    for &(page, event) in events {
        if !states.contains_key(&page) {
            if states.len() == state_slots {
                state_slots = (state_slots * 2).max(4);
                if !reserve(
                    &mut remaining,
                    state_slots,
                    size_of::<(usize, ContentState)>(),
                ) {
                    return (issues, true);
                }
            }
            states.insert(page, false);
        }
        if states[&page] || event == 2 {
            continue;
        }
        let issue = if event == 1 {
            states.insert(page, true);
            QualityIssue::UnsupportedInlineImage { page }
        } else {
            if seen.contains(&page) {
                continue;
            }
            if seen.len() >= seen_slots / 2 {
                seen_slots = (seen_slots * 2).max(4);
                if !reserve(
                    &mut remaining,
                    seen_slots,
                    size_of::<Option<(usize, u32)>>(),
                ) {
                    return (issues, true);
                }
            }
            seen.insert(page);
            form(page)
        };
        if issues.len() == output_slots {
            output_slots = (output_slots * 2).max(4);
            if !reserve(&mut remaining, output_slots, size_of::<QualityIssue>()) {
                return (issues, true);
            }
        }
        issues.push(issue);
    }
    (issues, false)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    /// 容量、交互ページ、重複、inline停止の任意履歴が独立モデルの完了prefixに一致する。
    #[test]
    fn scan_matches_capacity_and_history_model(events in prop::collection::vec((0usize..6, 0u8..3), 0..50), capacity in 0usize..6000) {
        let contents = events.iter().map(|&(page, event)| (page, match event { 0 => b"/F Do".to_vec(), 1 => b"BI".to_vec(), _ => b"q Q".to_vec() })).collect::<Vec<_>>();
        let (expected, stopped) = model(&events, capacity);
        let (issues, result) = run(&contents, capacity);
        prop_assert_eq!(issues, expected);
        prop_assert_eq!(result.is_err(), stopped);
        if stopped { prop_assert!(matches!(result, Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", .. })), "metadata capacity error"); }
    }
}
