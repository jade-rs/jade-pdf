//! CMap の lookup record と code space 保存容量を確保前に累積制限する回帰。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// lookup record の二つの String header と後勝ちの順序番号の要求サイズ。
const ENTRY_BYTES: u64 = std::mem::size_of::<(String, String, usize)>() as u64;
/// code space の幅・開始・終了値の要求サイズ。
const RANGE_BYTES: u64 = std::mem::size_of::<(usize, u32, u32)>() as u64;

/// 保存配列だけの上限を指定した解析予算を作る。
fn storage_budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_tounicode_storage_bytes: limit,
        ..Default::default()
    })
}

/// 保存容量超過の資源名と正確な予約量を検査する。
fn assert_storage_limit(budget: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "ToUnicode storage bytes", limit: found, actual: observed
    }) if found == limit && observed == actual));
}

/// bfchar・scalar/array bfrange の初回確保は、保存予算0なら始めない。
#[test]
fn storage_zero_rejects_every_mapping_kind_before_insertion() {
    for source in [
        b"1 beginbfchar\n<01> <0041>\nendbfchar".as_slice(),
        b"1 beginbfrange\n<01> <01> <0041>\nendbfrange".as_slice(),
        b"1 beginbfrange\n<01> <01> [<0041>]\nendbfrange".as_slice(),
    ] {
        let budget = storage_budget(0);
        assert!(parse_tounicode_cmap(&budget, source).is_empty());
        assert!(budget.is_stopped());
        assert_storage_limit(&budget, 0, ENTRY_BYTES);
    }
}

/// 初回1件の保存容量は inclusive とし、1 byte 不足なら entry を残さない。
#[test]
fn storage_boundary_is_inclusive() {
    let source = b"1 beginbfchar\n<01> <0041>\nendbfchar";
    let allowed = storage_budget(ENTRY_BYTES);
    assert_eq!(parse_tounicode_cmap(&allowed, source).mappings["01"], "A");
    assert!(allowed.check().is_ok());
    let denied = storage_budget(ENTRY_BYTES - 1);
    assert!(parse_tounicode_cmap(&denied, source).is_empty());
    assert_storage_limit(&denied, ENTRY_BYTES - 1, ENTRY_BYTES);
}

/// 保存配列の再確保は新容量全体を累積に数え、超過時には部分mapを破棄する。
#[test]
fn storage_growth_is_cumulative_before_reallocation() {
    let budget = storage_budget(3 * ENTRY_BYTES);
    let cmap = parse_tounicode_cmap(
        &budget,
        b"3 beginbfchar\n<01> <0041>\n<02> <0042>\n<03> <0043>\nendbfchar",
    );
    assert!(cmap.is_empty());
    assert_eq!(cmap.mappings.get("03"), None);
    assert_storage_limit(&budget, 3 * ENTRY_BYTES, 7 * ENTRY_BYTES);
}

/// 重複 key の後勝ちを保持しつつ、完了時の重複除去で予約量を返さない。
#[test]
fn duplicates_and_reparse_do_not_refund_storage() {
    let budget = storage_budget(4 * ENTRY_BYTES);
    let source = b"2 beginbfchar\n<01> <0041>\n<01> <0042>\nendbfchar";
    let first = parse_tounicode_cmap(&budget, source);
    assert_eq!(first.mappings.len(), 1);
    assert_eq!(first.mappings["01"], "B");
    let second = parse_tounicode_cmap(&budget, source);
    assert!(second.is_empty());
    assert_eq!(first.mappings["01"], "B");
    assert_storage_limit(&budget, 4 * ENTRY_BYTES, 6 * ENTRY_BYTES);
}

/// code space の重複も構築中の保存容量を消費し、完成後は一範囲へ整理する。
#[test]
fn code_spaces_share_the_storage_budget() {
    let source =
        b"0 beginbfchar\nendbfchar\n2 begincodespacerange\n<00> <FF>\n<00> <FF>\nendcodespacerange";
    let allowed = storage_budget(3 * RANGE_BYTES);
    let cmap = parse_tounicode_cmap(&allowed, source);
    assert_eq!(cmap.code_space_ranges.len(), 1);
    assert!(cmap.key_is_allowed("01"));
    assert!(!cmap.key_is_allowed("0001"));
    assert!(allowed.check().is_ok());
    let denied = storage_budget(RANGE_BYTES);
    assert_eq!(
        parse_tounicode_cmap(&denied, source)
            .code_space_ranges
            .len(),
        0
    );
    assert_storage_limit(&denied, RANGE_BYTES, 3 * RANGE_BYTES);
}

/// 無効 Unicode・逆向き code space は保存配列を確保しない。
#[test]
fn invalid_entries_need_no_storage() {
    let budget = storage_budget(0);
    let cmap = parse_tounicode_cmap(&budget, b"1 begincodespacerange\n<FF> <00>\nendcodespacerange\n1 beginbfchar\n<01> <D800>\nendbfchar");
    assert!(cmap.is_empty());
    assert!(cmap.code_space_ranges.is_empty());
    assert!(budget.check().is_ok());
}

/// 共有 clone は追加保存容量を要求せず、独立した解析は新しく加算する。
#[test]
fn clones_share_storage_and_new_parses_charge_it() {
    let budget = storage_budget(ENTRY_BYTES);
    let source = b"1 beginbfchar\n<01> <0041>\nendbfchar";
    let first = parse_tounicode_cmap(&budget, source);
    for _ in 0..32 {
        assert!(first.shares_storage_with(&first.clone()));
    }
    assert!(budget.check().is_ok());
    assert!(parse_tounicode_cmap(&budget, source).is_empty());
    assert_storage_limit(&budget, ENTRY_BYTES, 2 * ENTRY_BYTES);
    let fresh = storage_budget(ENTRY_BYTES);
    assert_eq!(parse_tounicode_cmap(&fresh, source).mappings["01"], "A");
    assert!(fresh.check().is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 入力順・重複にかかわらず、後勝ち lookup と key の整列は逐次 map と一致する。
    #[test]
    fn sorted_lookup_matches_last_assignment(entries in prop::collection::vec((0u8..32, 0x20u8..0x7f), 0..64)) {
        let mut source = format!("{} beginbfchar\n", entries.len());
        let mut expected = BTreeMap::new();
        for &(key, value) in &entries {
            source.push_str(&format!("<{key:02X}> <{value:04X}>\n"));
            expected.insert(format!("{key:02X}"), char::from(value).to_string());
        }
        source.push_str("endbfchar");
        let budget = DecodeBudget::default();
        let cmap = parse_tounicode_cmap(&budget, source.as_bytes());
        let observed = cmap.mappings.iter().map(|(k,v)| (k.clone(),v.clone())).collect::<BTreeMap<_,_>>();
        prop_assert_eq!(observed, expected);
        let keys = cmap.mappings.keys().collect::<Vec<_>>();
        prop_assert!(keys.windows(2).all(|w| w[0] < w[1]));
        prop_assert!(budget.check().is_ok());
    }
}

/// stream 統合で順序番号を振り直し、後の stream の重複 key を優先する。
#[test]
fn merged_streams_preserve_last_assignment() {
    let budget = DecodeBudget::default();
    let mut first = parse_tounicode_cmap_from_bytes(
        &budget,
        b"2 beginbfchar\n<01> <0041>\n<01> <0042>\nendbfchar",
    );
    let later = parse_tounicode_cmap_from_bytes(&budget, b"1 beginbfchar\n<01> <0043>\nendbfchar");
    assert!(first.extend(&budget, later).is_some());
    assert_eq!(first.finish(&budget).mappings["01"], "C");
    assert!(budget.check().is_ok());
}

/// 統合先の再確保も予算に含め、超過時には部分結果を破棄する。
#[test]
fn merged_streams_charge_destination_storage() {
    let budget = storage_budget(2 * ENTRY_BYTES);
    let mut first =
        parse_tounicode_cmap_from_bytes(&budget, b"1 beginbfchar\n<01> <0041>\nendbfchar");
    let later = parse_tounicode_cmap_from_bytes(&budget, b"1 beginbfchar\n<02> <0042>\nendbfchar");
    assert!(first.extend(&budget, later).is_none());
    let cmap = first.finish(&budget);
    assert!(cmap.is_empty());
    assert_eq!(cmap.mappings.get("02"), None);
    assert_storage_limit(&budget, 2 * ENTRY_BYTES, 4 * ENTRY_BYTES);
}
