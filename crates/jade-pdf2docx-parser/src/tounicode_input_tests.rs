//! entry のない CMap を含む入力走査と置換容量の累積制限。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// CMap 入力だけの小さな上限を作る。
fn input_budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_tounicode_input_bytes: limit,
        ..Default::default()
    })
}

/// 入力予算の資源名と累積予約量を検査する。
fn assert_input_limit(budget: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "ToUnicode input bytes", limit: found, actual: observed
    }) if found == limit && observed == actual));
}

/// entry がなくても CMap 選択の入力サイズを走査前に検査する。
#[test]
fn no_entry_selection_is_bounded() {
    let source = vec![b' '; 8192];
    let budget = input_budget(8191);
    assert!(parse_tounicode_cmap(&budget, &source).is_empty());
    assert!(budget.is_stopped());
    assert_input_limit(&budget, 8191, 8192);
}

/// 空 section と無効 operand だけの本文も入力予算を消費する。
#[test]
fn no_entry_body_is_bounded() {
    for source in [
        b"0 beginbfchar\nendbfchar".as_slice(),
        b"junk <ZZ> <QQ>".as_slice(),
    ] {
        let budget = input_budget(source.len() as u64 - 1);
        assert!(
            parse_tounicode_cmap_from_bytes(&budget, source)
                .finish(&budget)
                .is_empty()
        );
        assert_input_limit(&budget, source.len() as u64 - 1, source.len() as u64);
    }
}

/// 本文走査の inclusive 境界と、同じ入力の再解析の累積を固定する。
#[test]
fn body_boundary_and_reparse_are_cumulative() {
    let source = b"1 beginbfchar\n<01> <0041>\nendbfchar";
    let budget = input_budget(source.len() as u64);
    assert_eq!(
        parse_tounicode_cmap_from_bytes(&budget, source)
            .finish(&budget)
            .mappings["01"],
        "A"
    );
    assert!(budget.check().is_ok());
    assert!(
        parse_tounicode_cmap_from_bytes(&budget, source)
            .finish(&budget)
            .is_empty()
    );
    assert_input_limit(&budget, source.len() as u64, source.len() as u64 * 2);
    let fresh = input_budget(source.len() as u64);
    assert_eq!(
        parse_tounicode_cmap_from_bytes(&fresh, source)
            .finish(&fresh)
            .mappings["01"],
        "A"
    );
    assert!(fresh.check().is_ok());
}

/// 選択と本文の別走査をそれぞれ数え、止まった予算をエラー取得後も再開しない。
#[test]
fn selection_and_body_share_budget_and_stop() {
    let source = b"1 beginbfchar\n<01> <0041>\nendbfchar";
    let limit = source.len() as u64 * 2;
    let allowed = input_budget(limit);
    assert_eq!(parse_tounicode_cmap(&allowed, source).mappings["01"], "A");
    assert!(allowed.check().is_ok());
    let denied = input_budget(limit - 1);
    assert!(parse_tounicode_cmap(&denied, source).is_empty());
    assert_input_limit(&denied, limit - 1, limit);
    assert!(parse_tounicode_cmap(&denied, source).is_empty());
    assert!(denied.is_stopped());
    assert!(denied.check().is_ok());
}

/// UTF-8 置換は走査分と正確な置換後容量を確保前に累積予約する。
#[test]
fn invalid_utf8_replacement_capacity_is_bounded() {
    let source = b"1 beginbfchar\n<01> <0041> %\xff\nendbfchar";
    let length = source.len() as u64;
    let needed = length + String::from_utf8_lossy(source).len() as u64;
    let allowed = input_budget(needed);
    assert_eq!(
        parse_tounicode_cmap_from_bytes(&allowed, source)
            .finish(&allowed)
            .mappings["01"],
        "A"
    );
    assert!(allowed.check().is_ok());
    let denied = input_budget(needed - 1);
    assert!(
        parse_tounicode_cmap_from_bytes(&denied, source)
            .finish(&denied)
            .is_empty()
    );
    assert_input_limit(&denied, needed - 1, needed);
}

/// 未圧縮 stream の取得は元 PDF の領域を借用し、大きな本文を複製しない。
#[test]
fn uncompressed_cmap_payload_is_borrowed() {
    let source = b"1 0 obj\n<< /Length 33 >>\nstream\n1 beginbfchar\n<01> <0041>\nendbfchar\nendstream\nendobj";
    let payload = extract_stream_payload_for_object(&DecodeBudget::default(), source, 1).unwrap();
    let offset = source
        .windows(13)
        .position(|w| w == b"1 beginbfchar")
        .unwrap();
    assert_eq!(payload.as_ptr(), source[offset..].as_ptr());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 空白入力の反復走査は entry 予算に依存せず、独立した byte モデルに一致する。
    #[test]
    fn empty_scans_follow_byte_model(length in 1usize..4096, scans in 1u64..6, short in any::<bool>()) {
        let input = vec![b' '; length];
        let total = length as u64 * scans;
        let limit = total - u64::from(short);
        let budget = input_budget(limit);
        for _ in 0..scans { prop_assert!(parse_tounicode_cmap(&budget, &input).is_empty()); }
        if short { assert_input_limit(&budget, limit, total); }
        else { prop_assert!(budget.check().is_ok()); }
    }
}
