//! ToUnicode共有本体を用途横断の累積容量へ含める境界と停止を検証する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 一件のASCII文字対応を含む本文。
const SOURCE: &[u8] = b"1 beginbfchar\n<01> <0041>\nendbfchar";
/// 文字対応配列一件と二つの文字列payloadの要求容量。
const PAYLOAD_BYTES: u64 = (std::mem::size_of::<(String, String, usize)>() + 3) as u64;
/// 二つのVec headerとArcの二つのcounterを含む共有本体の要求容量。
const BODY_BYTES: u64 =
    (2 * std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>()) as u64;

/// 個別用途の上限を変えず共通容量だけを指定する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_allocation_bytes: limit,
        ..Default::default()
    })
}

/// 最初の容量エラーとエラー取得後も持続する停止を確認する。
fn exceeded(budget: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "total allocation bytes", limit: found, actual: total,
    }) if found == limit && total == actual));
    assert!(budget.is_stopped());
    assert!(parse_tounicode_body(budget, SOURCE).is_empty());
}

/// 非空mapはpayloadだけでなく共有本体もinclusive境界へ含める。
#[test]
fn owned_allocation_cmap_body_boundary() {
    let total = PAYLOAD_BYTES + BODY_BYTES;
    let allowed = budget(total);
    let cmap = parse_tounicode_body(&allowed, SOURCE);
    assert_eq!(cmap.mappings["01"], "A");
    assert!(allowed.check().is_ok());
    assert!(allowed.try_vector_capacity::<u8>(1).is_none());
    exceeded(&allowed, total, total + 1);
    let denied = budget(total - 1);
    assert!(parse_tounicode_body(&denied, SOURCE).is_empty());
    exceeded(&denied, total - 1, total);
}

/// 文字対応なしでもcode spaceを持つ本文は共有本体を要求する。
#[test]
fn owned_allocation_codespace_only_body_boundary() {
    let source = b"1 begincodespacerange\n<00> <FF>\nendcodespacerange";
    let total = BODY_BYTES + std::mem::size_of::<(usize, u32, u32)>() as u64;
    let allowed = budget(total);
    assert_eq!(
        parse_tounicode_body(&allowed, source)
            .code_space_ranges
            .len(),
        1
    );
    assert!(allowed.check().is_ok());
    let denied = budget(total - 1);
    assert!(
        parse_tounicode_body(&denied, source)
            .code_space_ranges
            .is_empty()
    );
    exceeded(&denied, total - 1, total);
}

/// 文書の共有hitとcloneは本体を再予約せず、停止後はcacheを返さない。
#[test]
fn owned_allocation_global_cache_shares_body_within_document() {
    let total = PAYLOAD_BYTES + BODY_BYTES;
    let budget = budget(total);
    budget.cache_streams(SOURCE, &[]);
    let original = parse_tounicode_body(&budget, SOURCE);
    budget.cache_global_cmap(SOURCE, original.clone());
    assert_eq!(original.mappings["01"], "A");
    for _ in 0..64 {
        assert!(original.shares_storage_with(&parse_tounicode_cmap(&budget, SOURCE)));
        assert!(original.shares_storage_with(&original.clone()));
    }
    assert!(budget.check().is_ok());
    assert!(budget.try_vector_capacity::<u8>(1).is_none());
    exceeded(&budget, total, total + 1);
    assert!(budget.cached_global_cmap(SOURCE).is_none());
}

/// 保存予算で途中停止した本文は部分mapの共有確保を開始しない。
#[test]
fn owned_allocation_stopped_cmap_discards_partial_result() {
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_tounicode_entries: 1,
        ..Default::default()
    });
    let cmap = parse_tounicode_body(
        &budget,
        b"2 beginbfchar\n<01> <0041>\n<02> <0042>\nendbfchar",
    );
    assert!(cmap.is_empty());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "ToUnicode entries",
            limit: 1,
            actual: 2,
        })
    ));
    assert!(parse_tounicode_body(&budget, SOURCE).is_empty());
}

/// 空本文・無効対応の反復解析は共通容量0でも成功する。
#[test]
fn owned_allocation_empty_cmaps_need_no_budget() {
    let budget = budget(0);
    for _ in 0..64 {
        for source in [
            b"".as_slice(),
            b"0 beginbfchar\nendbfchar",
            b"1 beginbfchar\n<01> <D800>\nendbfchar",
        ] {
            let cmap = parse_tounicode_body(&budget, source);
            assert!(cmap.shares_storage_with(&ToUnicodeCMap::default()));
        }
    }
    assert!(budget.check().is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 独立byteモデルで別用途との合算・解放後再作成・文書予算の独立性を確認する。
    #[test]
    fn owned_allocation_cmap_recreation_is_cumulative(
        count in 1usize..8, prefix in 0usize..32, short in any::<bool>(),
    ) {
        let total = prefix as u64 + count as u64 * (PAYLOAD_BYTES + BODY_BYTES);
        let limit = total - u64::from(short);
        let state = budget(limit);
        drop(state.metadata_vector::<u8>(prefix).unwrap());
        for index in 0..count {
            let cmap = parse_tounicode_body(&state, SOURCE);
            prop_assert_eq!(cmap.is_empty(), short && index + 1 == count);
        }
        if short { exceeded(&state, limit, total); }
        else {
            prop_assert!(state.check().is_ok());
            prop_assert!(state.try_vector_capacity::<u8>(1).is_none());
            exceeded(&state, limit, total + 1);
        }
        let fresh = budget(PAYLOAD_BYTES + BODY_BYTES);
        prop_assert_eq!(&parse_tounicode_body(&fresh, SOURCE).mappings["01"], "A");
        prop_assert!(fresh.check().is_ok());
    }
}
