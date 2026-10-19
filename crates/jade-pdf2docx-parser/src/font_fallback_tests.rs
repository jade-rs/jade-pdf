//! Font全体fallbackの保存容量を上流索引と独立に検査する。
use super::*;
use crate::{ParseError, ParseLimits};

/// fallback保存だけを制限する予算。
fn budget(bytes: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: bytes as u64,
        ..Default::default()
    })
}

/// 容量超過が公開エラーとして残ることを確かめる。
fn assert_limit(target: &DecodeBudget) {
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 空の値でもresource名とmap slotの保存容量は必要。
#[test]
fn zero_capacity_rejects_font_fallback_storage() {
    let target = budget(0);
    let result = build(&target, "/F 1 0 R", [(1, 0, String::new())]);
    assert!(result.is_empty());
    assert_limit(&target);
}

/// 保存済みmapを解放しても再作成分を累積する。
#[test]
fn recreation_charges_font_fallback_again() {
    let one = 4 * std::mem::size_of::<Option<(String, String)>>() + 2;
    let target = budget(2 * one - 1);
    assert!(!build(&target, "/F 1 0 R", [(1, 0, "A".to_owned())]).is_empty());
    assert!(build(&target, "/F 1 0 R", [(1, 0, "A".to_owned())]).is_empty());
    assert_limit(&target);
}

/// 一件保存後の容量超過でも部分mapを公開しない。
#[test]
fn later_failure_discards_completed_fallback_entries() {
    let one = 4 * std::mem::size_of::<Option<(String, String)>>() + 2;
    let target = budget(one);
    let result = build(&target, "/F 1 0 R /G 1 0 R", [(1, 0, "A".to_owned())]);
    assert!(result.is_empty());
    assert_limit(&target);
}

/// 正確な保存容量では成功し、1byte不足なら部分結果を捨てる。
#[test]
fn exact_capacity_counts_utf8_aliases_and_values() {
    let source = "/#e9 1 0 R /Alias 1 0 R /Alias 1 0 R";
    let one = 4 * std::mem::size_of::<Option<(String, String)>>() + 2 + 5 + 5 + 3 * "日本語".len();

    let target = budget(one - 1);
    assert!(build(&target, source, [(1, 0, "日本語".to_owned())]).is_empty());
    assert_limit(&target);
    let target = budget(one);
    let result = build(&target, source, [(1, 0, "日本語".to_owned())]);
    assert_eq!(result.get("é").map(String::as_str), Some("日本語"));
    assert_eq!(result.get("Alias").map(String::as_str), Some("日本語"));
    target.check().unwrap();
}

/// 同名resourceは既存のobject番号順の後出しを維持する。
#[test]
fn duplicate_alias_uses_later_object_value() {
    let target = budget(4 * std::mem::size_of::<Option<(String, String)>>() + 4);
    let result = build(
        &target,
        "/F 1 0 R /F 2 0 R",
        [(1, 0, "A".to_owned()), (2, 0, "B".to_owned())],
    );
    assert_eq!(result.get("F").map(String::as_str), Some("B"));
    target.check().unwrap();
}

/// 不正名は参照値と共に除外し、前の有効名へ誤って対応させない。
#[test]
fn invalid_names_are_not_rebound_to_earlier_names() {
    let target = budget(0);
    assert!(build(&target, "/Good /#gg 1 0 R", [(1, 0, "A".to_owned())]).is_empty());
    target.check().unwrap();
}

/// CMap別名は完成payloadを共有し、metadata slotと名だけを数える。
#[test]
fn cmap_aliases_share_payload_without_tounicode_recharge() {
    let payload = crate::text::parse_tounicode_body(
        &DecodeBudget::default(),
        b"1 beginbfchar\n<41> <0041>\nendbfchar",
    );
    assert!(!payload.is_empty());
    let one = 4 * std::mem::size_of::<Option<(String, crate::text::ToUnicodeCMap)>>() + 6;
    let target = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: one as u64,
        max_total_tounicode_bytes: 0,
        max_total_tounicode_storage_bytes: 0,
        ..Default::default()
    });
    let result = build(&target, "/F 1 0 R /Alias 1 0 R", [(1, 0, payload.clone())]);
    assert!(result["F"].shares_storage_with(&payload));
    assert!(result["Alias"].shares_storage_with(&payload));
    target.check().unwrap();
    let target = budget(one - 1);
    assert!(build(&target, "/F 1 0 R /Alias 1 0 R", [(1, 0, payload)]).is_empty());
    assert_limit(&target);
}

/// エラーを消費した後も停止したfallbackは入力iteratorを開始しない。
#[test]
fn stopped_fallback_does_not_start_values_after_consumed_error() {
    let target = budget(0);
    assert!(target.charge_metadata_workspace_bytes(1).is_none());
    assert_limit(&target);
    let values = std::iter::from_fn(|| -> Option<(u32, u16, String)> {
        panic!("stopped iterator");
    });
    assert!(build(&target, "/F 1 0 R", values).is_empty());
}

/// object番号/世代の全桁境界を完全tokenとして扱う。
#[test]
fn reference_tokens_handle_full_number_and_generation_ranges() {
    for number in [0, 1, 9, 10, 99, 100, u32::MAX] {
        for generation in [0, 1, u16::MAX] {
            let source = format!("/F {number} {generation} R");
            let target = DecodeBudget::default();
            assert_eq!(
                build(&target, &source, [(number, generation, "A".to_owned())])
                    .get("F")
                    .map(String::as_str),
                Some("A")
            );
            target.check().unwrap();
        }
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 別名/上書き履歴の独立容量モデルで境界と解放後の再作成を確認する。
    #[test]
    fn alias_history_matches_cumulative_capacity_model(
        aliases in prop::collection::vec(0usize..12, 1..48),
        value in ".{0,24}",
    ) {
        let mut source = String::new();
        let mut seen = [false; 12];
        let mut distinct = 0;
        let mut capacity = 0;
        let mut bytes = 0;
        for &alias in &aliases {
            let name = format!("F{alias}");
            source.push_str(&format!("/{name} 1 0 R "));
            bytes += name.len() + value.len();
            if !seen[alias] {
                if distinct >= capacity / 2 {
                    capacity = (capacity * 2).max(4);
                    bytes += capacity * std::mem::size_of::<Option<(String, String)>>();
                }
                seen[alias] = true;
                distinct += 1;
            }
        }

            let target = budget(bytes - 1);
            prop_assert!(build(&target, &source, [(1, 0, value.clone())]).is_empty());
            assert_limit(&target);
            let target = budget(2 * bytes - 1);
            let first = build(&target, &source, [(1, 0, value.clone())]);
            for (alias, &present) in seen.iter().enumerate() {
                if present { prop_assert_eq!(first.get(format!("F{alias}").as_str()), Some(&value)); }
            }
            drop(first);
            prop_assert!(build(&target, &source, [(1, 0, value.clone())]).is_empty());
            assert_limit(&target);
            let target = budget(2 * bytes);
            for _ in 0..2 {
                prop_assert!(!build(&target, &source, [(1, 0, value.clone())]).is_empty());
            }
            target.check().unwrap();

    }
}
