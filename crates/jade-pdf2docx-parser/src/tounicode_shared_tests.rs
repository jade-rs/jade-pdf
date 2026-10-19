//! 完成 CMap の clone が map/key/value/code space を複製しないことを検証する。
use super::*;

/// code space と一対多 Unicode を持つ小さな CMap。
const SOURCE: &[u8] = b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<01> <65E5672C>\nendbfchar";

/// clone は map node を含む同じ不変保存領域を参照する。
#[test]
fn clone_shares_complete_cmap_storage() {
    let budget = DecodeBudget::default();
    let original = parse_tounicode_cmap(&budget, SOURCE);
    let cloned = original.clone();
    assert!(original.shares_storage_with(&cloned));
    assert!(std::ptr::eq(
        original.mappings["01"].as_ptr(),
        cloned.mappings["01"].as_ptr()
    ));
    assert!(std::ptr::eq(
        original.code_space_ranges.as_ptr(),
        cloned.code_space_ranges.as_ptr()
    ));
    assert!(budget.check().is_ok());
}

/// 空 CMap も共通の不変領域を使い、default の反復で heap を増やさない。
#[test]
fn empty_cmaps_share_one_storage() {
    assert!(ToUnicodeCMap::default().shares_storage_with(&ToUnicodeCMap::default()));
}

/// 共有元を破棄しても Unicode と code space lookup は保持される。
#[test]
fn clone_survives_original_drop() {
    let cloned = {
        let original = parse_tounicode_cmap(&DecodeBudget::default(), SOURCE);
        original.clone()
    };
    assert_eq!(cloned.mappings["01"], "日本");
    assert!(cloned.key_is_allowed("01"));
    assert!(!cloned.key_is_allowed("0001"));
}

/// 独立した解析の可変構築状態は共有せず、以前の完成 map を変えない。
#[test]
fn independent_parses_preserve_prior_cmap() {
    let budget = DecodeBudget::default();
    let first = parse_tounicode_cmap(&budget, SOURCE);
    let second = parse_tounicode_cmap(&budget, b"1 beginbfchar\n<01> <0041>\nendbfchar");
    assert!(!first.shares_storage_with(&second));
    assert_eq!(first.mappings["01"], "日本");
    assert_eq!(second.mappings["01"], "A");
    assert!(budget.check().is_ok());
}
