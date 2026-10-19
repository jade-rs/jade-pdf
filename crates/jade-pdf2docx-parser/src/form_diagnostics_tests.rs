//! Font診断の順序・重複・全拡張容量・再作成累積と停止保持を検査する。
use super::*;
use crate::{ParseError, ParseLimits, UnsupportedFontReason};
use proptest::prelude::*;
use std::collections::BTreeSet;

/// metadata容量だけを制限する予算を作る。
fn budget(capacity: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity as u64,
        ..Default::default()
    })
}

/// 一つのFont選択をモデル用に作る。
fn selection(number: u32) -> Selection {
    Selection {
        object_number: Some(number),
        reason: UnsupportedFontReason::Type3,
    }
}

/// 集合slotの独立した要求byte数。
fn set_slot() -> usize {
    std::mem::size_of::<Option<(usize, Selection)>>()
}

/// 診断出力slotの独立した要求byte数。
fn issue_slot() -> usize {
    std::mem::size_of::<QualityIssue>()
}

/// 最初の集合/出力容量を含む境界と重複時の無確保を確認する。
#[test]
fn inclusive_capacity_and_duplicate_require_no_extra_storage() {
    let one = 4 * (set_slot() + issue_slot());
    for capacity in [one - 1, one] {
        let target = budget(capacity);
        let mut fonts = Fonts::default();
        let mut report = QualityReport::default();
        assert_eq!(
            fonts
                .record(&target, &mut report, 0, selection(1))
                .is_some(),
            capacity == one
        );
        assert_eq!(
            fonts
                .record(&target, &mut report, 0, selection(1))
                .is_some(),
            capacity == one
        );
        assert_eq!(report.issues.len(), usize::from(capacity == one));
        assert_eq!(target.check().is_ok(), capacity == one);
    }
}

/// 集合と出力の拡張失敗で、確保できない次の診断を公開しない。
#[test]
fn growth_failure_keeps_only_completed_diagnostics() {
    for capacity in [
        12 * set_slot() + 4 * issue_slot(),
        28 * set_slot() + 4 * issue_slot(),
    ] {
        let target = budget(capacity);
        let mut fonts = Fonts::default();
        let mut report = QualityReport::default();
        for number in 0..4 {
            fonts
                .record(&target, &mut report, 0, selection(number))
                .unwrap();
        }
        assert!(
            fonts
                .record(&target, &mut report, 0, selection(4))
                .is_none()
        );
        assert_eq!(report.issues.len(), 4);
        assert!(matches!(
            target.check(),
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                ..
            })
        ));
    }
}

/// 解放後の再作成は累積し、停止エラーを消費しても再開しない。
#[test]
fn recreation_is_cumulative_and_stopped_duplicate_is_rejected() {
    let one = 4 * (set_slot() + issue_slot());
    for capacity in [2 * one - 1, 2 * one] {
        let target = budget(capacity);
        for pass in 0..2 {
            let mut fonts = Fonts::default();
            let mut report = QualityReport::default();
            assert_eq!(
                fonts
                    .record(&target, &mut report, 0, selection(1))
                    .is_some(),
                pass == 0 || capacity == 2 * one
            );
            if pass == 1 && capacity < 2 * one {
                assert!(target.check().is_err());
                assert!(
                    fonts
                        .record(&target, &mut report, 0, selection(1))
                        .is_none()
                );
                assert!(report.issues.is_empty());
            }
        }
    }
}

/// ページ・参照番号・理由をすべて重複キーに含め、検出順を保つ。
#[test]
fn page_and_selection_identity_preserve_detection_order() {
    let target = budget(12 * set_slot() + 4 * issue_slot());
    let mut fonts = Fonts::default();
    let mut report = QualityReport::default();
    for (page, selected) in [
        (1, selection(3)),
        (0, selection(3)),
        (1, selection(3)),
        (
            1,
            Selection {
                object_number: None,
                reason: UnsupportedFontReason::Subtype,
            },
        ),
    ] {
        fonts.record(&target, &mut report, page, selected).unwrap();
    }
    assert_eq!(
        report.issues,
        [
            QualityIssue::UnsupportedFont {
                page: 1,
                object_number: Some(3),
                reason: UnsupportedFontReason::Type3
            },
            QualityIssue::UnsupportedFont {
                page: 0,
                object_number: Some(3),
                reason: UnsupportedFontReason::Type3
            },
            QualityIssue::UnsupportedFont {
                page: 1,
                object_number: None,
                reason: UnsupportedFontReason::Subtype
            }
        ]
    );
    target.check().unwrap();
}

/// 既存診断を移動して出力を拡張し、既存の検出順を変えない。
#[test]
fn shared_report_growth_preserves_existing_issues() {
    let target = budget(4 * set_slot() + 8 * issue_slot());
    let mut report = QualityReport {
        issues: Vec::with_capacity(4),
    };
    for page in 0..4 {
        report
            .issues
            .push(QualityIssue::UnsupportedInlineImage { page });
    }
    let before = report.issues.clone();
    Fonts::default()
        .record(&target, &mut report, 0, selection(7))
        .unwrap();
    assert_eq!(&report.issues[..4], &before);
    assert_eq!(report.issues.len(), 5);
    target.check().unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 任意の選択列で容量予約・出力順・重複・停止が独立モデルと一致する。
    #[test]
    fn diagnostics_match_capacity_and_order_model(
        operations in prop::collection::vec((0usize..3, 0u32..12, any::<bool>()), 0..64),
        capacity in 0usize..6000,
        builds in 1usize..4,
    ) {
        let target = budget(capacity);
        let mut spent = 0;
        let mut stopped = false;
        for _ in 0..builds {
            let mut fonts = Fonts::default();
            let mut report = QualityReport::default();
            let mut seen = BTreeSet::new();
            let mut expected = Vec::new();
            let mut set_capacity = 0usize;
            let mut report_capacity = 0usize;
            for &(page, number, other) in &operations {
                let selected = Selection { object_number: Some(number), reason: if other { UnsupportedFontReason::Subtype } else { UnsupportedFontReason::Type3 } };
                if !stopped && !seen.contains(&(page, selected)) {
                    if seen.len() >= set_capacity / 2 {
                        let next = (set_capacity * 2).max(4);
                        if spent + next * set_slot() > capacity { stopped = true; }
                        else { spent += next * set_slot(); set_capacity = next; }
                    }
                    if !stopped && expected.len() == report_capacity {
                        let next = (report_capacity * 2).max(4);
                        if spent + next * issue_slot() > capacity { stopped = true; }
                        else { spent += next * issue_slot(); report_capacity = next; }
                    }
                    if !stopped {
                        seen.insert((page, selected));
                        expected.push(QualityIssue::UnsupportedFont { page, object_number: selected.object_number, reason: selected.reason });
                    }
                }
                prop_assert_eq!(fonts.record(&target, &mut report, page, selected).is_some(), !stopped);
                prop_assert_eq!(&report.issues, &expected);
            }
        }
        prop_assert_eq!(target.check().is_err(), stopped);
    }
}
