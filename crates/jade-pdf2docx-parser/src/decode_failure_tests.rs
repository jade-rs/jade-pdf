//! 停止後の深度検査が最初の失敗と停止状態を保持することを検証する。
use super::*;

/// 両 Form 深度検査を同じ失敗履歴で検証するための関数型。
type DepthCheck = fn(&DecodeBudget, usize) -> Option<()>;

/// 本文 walker と q/Q stack の深度検査を返す。
fn depth_checks() -> [DepthCheck; 2] {
    [
        DecodeBudget::check_form_depth,
        DecodeBudget::check_form_graphics_depth,
    ]
}

/// 別 extractor から遅れて届く超過報告も最初の失敗を変更しない。
#[test]
fn late_limit_reports_preserve_first_failure_and_stop() {
    let budget = DecodeBudget::default();
    budget.invalid_tounicode("first");
    budget.exceeded("later limit", 0, 1);
    assert!(matches!(
        budget.check(),
        Err(ParseError::InvalidToUnicode { reason: "first" })
    ));
    budget.exceeded("after delivery", 0, 1);
    assert!(budget.check().is_ok());
    assert!(budget.is_stopped());
}

/// mapping payload 超過は後続の Form 深度超過で上書きされない。
#[test]
fn depth_checks_preserve_first_resource_failure() {
    for check in depth_checks() {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_tounicode_bytes: 2,
            max_container_nesting: 1,
            ..Default::default()
        });
        assert_eq!(budget.charge_tounicode_bytes(3), None);
        assert_eq!(check(&budget, 2), None);
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "ToUnicode mapping bytes",
                limit: 2,
                actual: 3
            })
        ));
    }
}

/// 不正 CMap の理由は後続の深度超過で失われない。
#[test]
fn depth_checks_preserve_first_invalid_cmap_failure() {
    for check in depth_checks() {
        let budget = DecodeBudget::default();
        budget.invalid_tounicode("first invalid CMap");
        assert_eq!(check(&budget, usize::MAX), None);
        assert!(matches!(
            budget.check(),
            Err(ParseError::InvalidToUnicode {
                reason: "first invalid CMap"
            })
        ));
    }
}

/// allocation 失敗も最初の原因として公開入口へ伝わる。
#[test]
fn depth_checks_preserve_first_allocation_failure() {
    for check in depth_checks() {
        let budget = DecodeBudget::default();
        assert!(budget.try_string_capacity(usize::MAX).is_none());
        assert_eq!(check(&budget, usize::MAX), None);
        assert!(matches!(budget.check(), Err(ParseError::Io(error))
            if error.kind() == std::io::ErrorKind::OutOfMemory));
    }
}

/// 停止後は上限内の深度でも処理を続行させない。
#[test]
fn stopped_depth_checks_reject_shallow_stack() {
    for check in depth_checks() {
        let budget = DecodeBudget::default();
        budget.invalid_tounicode("stop");
        assert_eq!(check(&budget, 0), None);
        assert_eq!(check(&budget, 256), None);
        assert!(budget.is_stopped());
    }
}

/// 公開入口へエラーを渡した後も停止し、後続検査で新しいエラーを作らない。
#[test]
fn consumed_failure_is_not_recreated_by_depth_checks() {
    let budget = DecodeBudget::default();
    budget.invalid_tounicode("stop");
    assert!(budget.check().is_err());
    for check in depth_checks() {
        assert_eq!(check(&budget, usize::MAX), None);
        assert_eq!(check(&budget, 0), None);
        assert!(budget.check().is_ok());
        assert!(budget.is_stopped());
    }
}
