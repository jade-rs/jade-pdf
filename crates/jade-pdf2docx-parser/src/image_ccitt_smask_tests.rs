//! 1 bit CCITT外部maskの極性・寸法・確保境界を固定する。
use super::*;

/// 停止済みの予算はCCITT maskを確保せず、キャンセル理由を保持する。
#[test]
fn ccitt_mask_keeps_cancellation() {
    let token = crate::ParseCancellation::new();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    token.cancel();
    assert!(
        image_ccitt::decode_gray(&budget, b"", b"broken", 2, 1, b"<< /K -1 /Columns 2 >>")
            .is_none()
    );
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
}
