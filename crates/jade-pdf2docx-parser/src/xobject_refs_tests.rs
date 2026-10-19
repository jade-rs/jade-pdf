//! XObject の予算・名前・ページ順の回帰。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 指定した累積 metadata 容量だけを許可する。
fn budget(limit: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit as u64,
        ..Default::default()
    })
}

/// 要求全容量を公開エラーに保持することを検証する。
fn assert_limit(budget: &DecodeBudget, limit: usize, actual: usize) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: found_limit, actual: found_actual
    }) if found_limit == limit as u64 && found_actual == actual as u64));
}

/// 名前の Latin-1 対応と escape を保持し、UTF-8 全容量を一度だけ予約する。
#[test]
fn name_capacity_is_inclusive_and_cumulative() {
    let raw = b"A#80#ff";
    let expected = "A\u{80}\u{ff}";
    let one = expected.len();
    for limit in [one * 2 - 1, one * 2] {
        let budget = budget(limit);
        assert_eq!(resource_name(&budget, raw).as_deref(), Some(expected));
        let second = resource_name(&budget, raw);
        if limit == one * 2 {
            assert_eq!(second.as_deref(), Some(expected));
            assert!(budget.check().is_ok());
        } else {
            assert!(second.is_none());
            assert_limit(&budget, limit, one * 2);
            assert!(resource_name(&budget, b"A").is_none());
            assert!(budget.check().is_ok());
        }
    }
}

/// 不正な escape は出力を確保せず、空名は予算0でも空のキーを返す。
#[test]
fn invalid_names_do_not_allocate() {
    let budget = budget(0);
    for raw in [b"#".as_slice(), b"A#1", b"#G1", b"#1G", b"#00#x0"] {
        assert!(resource_name(&budget, raw).is_none());
    }
    assert_eq!(resource_name(&budget, b""), Some(String::new()));
    assert!(budget.check().is_ok());
}

/// 破棄する entry も配列容量へ数え、有効な名前 payload は別に予約する。
#[test]
fn reference_slots_and_name_payload_are_reserved_separately() {
    let indexing = DecodeBudget::default();
    let objects = crate::object_index::build(&indexing, b"7 0 obj << >> endobj").unwrap();
    let resources = b"<< /XObject << /A 7 0 R /#80 7 0 R /#G0 7 0 R /Bad 8 0 R >> >>";
    let slots = 4 * std::mem::size_of::<(String, u32)>();
    for (limit, actual) in [
        (slots - 1, slots),
        (slots, slots + 1),
        (slots + 2, slots + 3),
    ] {
        let budget = budget(limit);
        assert!(from_resources(&budget, &objects, resources).is_empty());
        assert_limit(&budget, limit, actual);
    }
    let budget = budget(slots + 3);
    assert_eq!(
        from_resources(&budget, &objects, resources),
        [("A".into(), 7), ("\u{80}".into(), 7)]
    );
    assert!(budget.check().is_ok());
}

/// 別呼び出しと解放した参照配列・名前 payload も累積予算から戻さない。
#[test]
fn repeated_resource_extraction_is_cumulative() {
    let indexing = DecodeBudget::default();
    let objects = crate::object_index::build(&indexing, b"7 0 obj << >> endobj").unwrap();
    let resources = b"<< /XObject << /A 7 0 R >> >>";
    let one = std::mem::size_of::<(String, u32)>() + 1;
    for limit in [2 * one - 1, 2 * one] {
        let budget = budget(limit);
        assert_eq!(
            from_resources(&budget, &objects, resources),
            [("A".into(), 7)]
        );
        let second = from_resources(&budget, &objects, resources);
        if limit == 2 * one {
            assert_eq!(second, [("A".into(), 7)]);
            assert!(budget.check().is_ok());
        } else {
            assert!(second.is_empty());
            assert_limit(&budget, limit, 2 * one);
        }
    }
}

/// 中間ページ列と最終出力を個別に予約し、別ページの同一 object を保持する。
#[test]
fn page_output_is_inclusive_and_preserves_order() {
    let source =
        b"1 0 obj << /Type /Page /Resources << /XObject << /A 7 0 R /#80 7 0 R >> >> >> endobj
        2 0 obj << /Type /Page /XObject << /B 7 0 R >> >> endobj
        7 0 obj << >> endobj";
    let indexing = budget(0);
    assert!(crate::object_index::build(&indexing, source).is_none());
    let Err(ParseError::ResourceLimit { actual, .. }) = indexing.check() else {
        panic!("索引容量がない")
    };
    let one = actual as usize
        + 2 * std::mem::size_of::<&[u8]>()
        + 2 * std::mem::size_of::<Vec<(String, u32)>>()
        + 3 * std::mem::size_of::<(String, u32)>()
        + 4
        + 3 * std::mem::size_of::<(usize, String, u32)>();
    for limit in [one - 1, one] {
        let budget = budget(limit);
        let result = page_references(&budget, source);
        if limit == one {
            assert_eq!(
                result,
                [
                    (0, "A".into(), 7),
                    (0, "\u{80}".into(), 7),
                    (1, "B".into(), 7)
                ]
            );
            assert!(budget.check().is_ok());
        } else {
            assert!(result.is_empty());
            assert_limit(&budget, limit, one);
            assert!(page_references(&budget, source).is_empty());
            assert!(budget.check().is_ok());
        }
    }
}

/// Root なしの間接 Resources・XObject・別名参照も有効世代で解決する。
#[test]
fn indirect_resources_and_generations_are_resolved() {
    let source = b"1 0 obj << /Type /Page /Resources 2 0 R >> endobj
        2 0 obj << /XObject 3 0 R >> endobj
        3 0 obj << /A#23 7 1 R /Bad 7 0 R /Inline << >> /Cycle 8 0 R >> endobj
        7 1 obj 9 0 R endobj
        8 0 obj 8 0 R endobj
        9 0 obj << /Type /XObject >> endobj";
    let budget = DecodeBudget::default();
    assert_eq!(page_references(&budget, source), [(0, "A#".into(), 9)]);
    assert!(budget.check().is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// 任意の byte 名を escape し、既存キーと UTF-8 要求容量の境界を照合する。
    #[test]
    fn borrowed_name_matches_existing_byte_key(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
        let raw = bytes.iter().map(|byte| format!("#{byte:02X}")).collect::<String>();
        let expected = metadata::resource_name(raw.as_bytes()).unwrap();
        let exact = budget(expected.len());
        prop_assert_eq!(resource_name(&exact, raw.as_bytes()), Some(expected.clone()));
        prop_assert!(exact.check().is_ok());
        if !expected.is_empty() {
            let short = budget(expected.len() - 1);
            prop_assert!(resource_name(&short, raw.as_bytes()).is_none());
            assert_limit(&short, expected.len() - 1, expected.len());
        }
    }

    /// 辞書 entry 数と名前の独立モデルで参照の保存容量と順序を検証する。
    #[test]
    fn resource_references_match_capacity_model(count in 0usize..24) {
        let indexing = DecodeBudget::default();
        let objects = crate::object_index::build(&indexing, b"7 0 obj << >> endobj").unwrap();
        let names = (0..count).map(|index| format!("N{index}")).collect::<Vec<_>>();
        let entries = names.iter().map(|name| format!(" /{name} 7 0 R")).collect::<String>();
        let dictionary = format!("<< /XObject << {entries} >> >>");
        let capacity = count * std::mem::size_of::<(String, u32)>() + names.iter().map(String::len).sum::<usize>();
        let exact = budget(capacity);
        let expected = names.into_iter().map(|name| (name, 7)).collect::<Vec<_>>();
        prop_assert_eq!(from_resources(&exact, &objects, dictionary.as_bytes()), expected);
        prop_assert!(exact.check().is_ok());
        if capacity > 0 {
            let short = budget(capacity - 1);
            prop_assert!(from_resources(&short, &objects, dictionary.as_bytes()).is_empty());
            assert_limit(&short, capacity - 1, capacity);
        }
    }
}
