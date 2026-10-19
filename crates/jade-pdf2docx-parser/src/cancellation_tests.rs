//! キャンセル検出後の停止保持と、既存の最初の失敗の優先順位を固定する。
use crate::{ParseCancellation, ParseError, ParseLimits, decode::DecodeBudget};

/// エラー取得後もキャンセルされた予算で処理を再開しない。
#[test]
fn cancellation_error_remains_terminal_after_observation() {
    let token = ParseCancellation::new();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(!budget.is_stopped());
    token.cancel();
    for _ in 0..3 {
        assert!(budget.is_stopped());
        assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
        assert!(budget.charge_metadata_workspace_bytes(1).is_none());
    }
}

/// 既に記録済みの容量超過を遅れて届いたキャンセルで上書きしない。
#[test]
fn first_resource_failure_precedes_later_cancellation() {
    let token = ParseCancellation::new();
    let limits = ParseLimits {
        max_total_metadata_workspace_bytes: 0,
        ..Default::default()
    };
    let budget = DecodeBudget::with_cancellation(&limits, Some(&token));
    assert!(budget.charge_metadata_workspace_bytes(1).is_none());
    token.cancel();
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    assert!(budget.is_stopped());
}

/// 異なるthreadのcloneで出された停止要求をsleepなしで受け取る。
#[test]
fn another_thread_can_cancel_the_shared_handle() {
    let token = ParseCancellation::new();
    let clone = token.clone();
    std::thread::spawn(move || clone.cancel()).join().unwrap();
    assert!(token.is_cancelled());
    assert!(!ParseCancellation::new().is_cancelled());
}
