//! 標準Font判定のPDF名復号にも確保前の容量制限を適用する。
use super::*;
use crate::{ParseError, ParseLimits, decode::DecodeBudget};

/// 長い名前でも、標準幅の判定前に保存容量超過を明示する。
#[test]
fn base_font_name_is_budgeted_before_decoding() {
    for resource in ["total allocation bytes", "metadata workspace bytes"] {
        let limits = ParseLimits {
            max_total_allocation_bytes: if resource == "total allocation bytes" {
                0
            } else {
                u64::MAX
            },
            max_total_metadata_workspace_bytes: if resource == "metadata workspace bytes" {
                0
            } else {
                u64::MAX
            },
            ..Default::default()
        };
        let budget = DecodeBudget::new(&limits);
        let dictionary = format!("<< /Subtype /Type1 /BaseFont /{} >>", "A".repeat(20000));
        Metrics::read(&Objects::new(), dictionary.as_bytes(), &budget, &[]);
        assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
            resource: found, limit: 0, actual: 20000,
        }) if found == resource));
        assert!(budget.is_stopped());
    }
}

/// escape復号後の容量境界と既存のCourier幅を保持する。
#[test]
fn escaped_standard_name_has_an_inclusive_capacity() {
    let limits = ParseLimits {
        max_total_allocation_bytes: 8,
        ..Default::default()
    };
    let budget = DecodeBudget::new(&limits);
    let metrics = Metrics::read(
        &Objects::new(),
        b"<< /Subtype /Type1 /BaseFont /C#6furier >>",
        &budget,
        &[],
    );
    let layout = metrics.layout(&budget, b"(A)", 10., 0., 0.).unwrap();
    assert_eq!(layout.advance, 6.);
    assert!(budget.try_vector_capacity::<u8>(1).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "total allocation bytes",
            actual: 9,
            ..
        })
    ));
}
