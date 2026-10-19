//! 埋め込みFont候補の保存容量と出力順序を検査する。
use super::*;
use crate::{ParseError, ParseLimits};
use std::cell::Cell;

/// metadata保存だけの境界を検査する予算を作る。
fn budget(capacity: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity as u64,
        ..Default::default()
    })
}

/// OS/2のinstallable embeddingを明示した最小sfnt fixtureを作る。
fn font(marker: u8) -> Vec<u8> {
    let mut bytes = vec![0; 39];
    bytes[..4].copy_from_slice(&[0, 1, 0, 0]);
    bytes[4..6].copy_from_slice(&1u16.to_be_bytes());
    bytes[12..16].copy_from_slice(b"OS/2");
    bytes[20..24].copy_from_slice(&28u32.to_be_bytes());
    bytes[24..28].copy_from_slice(&10u32.to_be_bytes());
    bytes[38] = marker;
    bytes
}

/// 名前と参照先だけを変えた最外FontDescriptorの列を作る。
fn descriptors(names: &[&str]) -> String {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            format!(
                "{} 0 obj << /Type /FontDescriptor /FontName /{name} /FontFile2 {} 0 R >> endobj\n",
                index + 1,
                index + 101
            )
        })
        .collect()
}

/// 独立した4/8/16/32容量モデルで、保存した候補全件のslot容量を求める。
fn slots(count: usize) -> usize {
    [4, 8, 16, 32]
        .into_iter()
        .filter(|&capacity| count > capacity / 2 || capacity == 4 && count > 0)
        .map(|capacity| capacity * std::mem::size_of::<(String, Vec<u8>, ToUnicodeCMap)>())
        .sum()
}

/// 最終結果は全候補分のslotを一度だけ予約する。
fn results(count: usize) -> usize {
    count * std::mem::size_of::<ParsedEmbeddedFont>()
}

/// 保存不足は公開APIと同じ型付きmetadataエラーにする。
fn assert_limit(target: &DecodeBudget) {
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// UTF-8名・候補・結果の正確な境界を累積し、解放しても返却しない。
#[test]
fn exact_capacity_and_recreation_are_cumulative() {
    let source = descriptors(&["Z", "#c3#a9"]);
    let one = "Zé".len() + slots(2) + results(2);
    for (limit, builds) in [(one - 1, 1), (one, 1), (2 * one - 1, 2), (2 * one, 2)] {
        let target = budget(limit);
        for pass in 0..builds {
            let fonts = build(
                &target,
                source.as_bytes(),
                |number| Some(font(number as u8)),
                |_| Default::default(),
            );
            if (pass + 1) * one <= limit {
                assert_eq!(
                    fonts
                        .iter()
                        .map(|font| font.font_name.as_str())
                        .collect::<Vec<_>>(),
                    ["Z", "é"]
                );
            } else {
                assert!(fonts.is_empty());
            }
        }
        assert_eq!(target.check().is_ok(), builds * one <= limit);
    }
}

/// 5件目の全新配列を予約できなければ完成済み候補も破棄する。
#[test]
fn candidate_growth_failure_discards_all_fonts() {
    let source = descriptors(&["A", "B", "C", "D", "E"]);
    let target = budget(5 + slots(4));
    assert!(
        build(
            &target,
            source.as_bytes(),
            |_| Some(font(0)),
            |_| Default::default()
        )
        .is_empty()
    );
    assert_limit(&target);
}

/// 同名の初出payloadを保持し、同名の後続streamやCMapを取得しない。
#[test]
fn duplicates_skip_payload_and_preserve_name_order() {
    let source = descriptors(&["Z", "A", "#5a", "M"]);
    let target = budget(4 + slots(3) + results(3));
    let mut loaded = Vec::new();
    let mut mapped = Vec::new();
    let fonts = build(
        &target,
        source.as_bytes(),
        |number| {
            loaded.push(number);
            Some(font(number as u8))
        },
        |number| {
            mapped.push(number);
            Default::default()
        },
    );
    assert_eq!(loaded, [101, 102, 104]);
    assert_eq!(mapped, [1, 2, 4]);
    assert_eq!(
        fonts
            .iter()
            .map(|font| font.font_name.as_str())
            .collect::<Vec<_>>(),
        ["A", "M", "Z"]
    );
    assert_eq!(
        fonts.iter().map(|font| font.bytes[38]).collect::<Vec<_>>(),
        [102, 104, 101]
    );
    target.check().unwrap();
}

/// 型・最外辞書・不正escape・不正UTF-8を検査し、不正UTF-8の破棄容量も累積する。
#[test]
fn discarded_names_are_bounded_without_payload_lookup() {
    let source = b"1 0 obj << /Type /Other /FontName /Other /FontFile2 2 0 R >> endobj\n2 0 obj << /Note << /Type /FontDescriptor >> /FontName /Nested /FontFile2 2 0 R >> endobj\n3 0 obj << /Type /FontDescriptor /FontName /#gg /FontFile2 2 0 R >> endobj\n4 0 obj << /Type /FontDescriptor /FontName /#ff /FontFile2 2 0 R >> endobj";
    for capacity in [0, 1] {
        let target = budget(capacity);
        assert!(
            build(
                &target,
                source,
                |_| panic!("invalid name loaded"),
                |_| panic!("invalid name mapped")
            )
            .is_empty()
        );
        assert_eq!(target.check().is_ok(), capacity == 1);
    }
}

/// 不正・未対応payloadは候補へ保存せず、名前の要求容量だけを加算する。
#[test]
fn invalid_payloads_use_no_candidate_or_result_slots() {
    let source = descriptors(&["A", "B", "C"]);
    let target = budget(3);
    assert!(
        build(
            &target,
            source.as_bytes(),
            |number| match number {
                101 => None,
                102 => Some(Vec::new()),
                _ => Some(b"invalid".to_vec()),
            },
            |_| panic!("invalid font mapped")
        )
        .is_empty()
    );
    target.check().unwrap();
}

/// 採用条件に達した入力順の32件だけを保存し、33件目を処理しない。
#[test]
fn count_limit_preserves_first_candidates_and_bounds_lookup() {
    let names: Vec<_> = (0..33)
        .rev()
        .map(|number| format!("F{number:02}"))
        .collect();
    let source = descriptors(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let target = budget(32 * 3 + slots(32) + results(32));
    let calls = Cell::new(0);
    let fonts = build(
        &target,
        source.as_bytes(),
        |_| {
            calls.set(calls.get() + 1);
            Some(font(0))
        },
        |_| Default::default(),
    );
    assert_eq!(calls.get(), 32);
    assert_eq!(fonts.len(), 32);
    assert_eq!(fonts.first().unwrap().font_name, "F01");
    assert_eq!(fonts.last().unwrap().font_name, "F32");
    target.check().unwrap();
}

/// payload/CMap取得中の停止でも先に完成した候補を公開しない。
#[test]
fn lookup_failure_discards_candidates_and_remains_stopped() {
    let source = descriptors(&["A", "B", "C"]);
    for stop_mapping in [false, true] {
        let target = budget(10000);
        let loaded = Cell::new(0);
        let mapped = Cell::new(0);
        let fonts = build(
            &target,
            source.as_bytes(),
            |number| {
                loaded.set(loaded.get() + 1);
                if !stop_mapping && number == 102 {
                    assert!(target.charge_metadata_workspace_bytes(10001).is_none());
                }
                Some(font(0))
            },
            |number| {
                mapped.set(mapped.get() + 1);
                if stop_mapping && number == 2 {
                    assert!(target.charge_metadata_workspace_bytes(10001).is_none());
                }
                Default::default()
            },
        );
        assert!(fonts.is_empty());
        assert_eq!(loaded.get(), 2);
        assert_eq!(mapped.get(), if stop_mapping { 2 } else { 1 });
        assert_limit(&target);
        assert!(
            build(
                &target,
                source.as_bytes(),
                |_| panic!("stopped payload lookup"),
                |_| panic!("stopped mapping lookup")
            )
            .is_empty()
        );
        target.check().unwrap();
        assert!(
            build(
                &target,
                source.as_bytes(),
                |_| panic!("consumed failure resumed payload"),
                |_| panic!("consumed failure resumed mapping")
            )
            .is_empty()
        );
    }
}

/// 後続Fontの修復停止は先に出力へ移した完成Fontも破棄する。
#[test]
fn repair_failure_discards_completed_output() {
    let source = descriptors(&["A", "Z"]);
    let cmap = crate::text::parse_tounicode_body(
        &DecodeBudget::default(),
        b"1 beginbfchar\n<01> <0041>\nendbfchar",
    );
    let target = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    // 最小cmap/head fixtureは修復のfont予算を必要とする。
    let mut repair = vec![0; 84];
    repair[..4].copy_from_slice(&[0, 1, 0, 0]);
    repair[4..6].copy_from_slice(&2u16.to_be_bytes());
    repair[12..16].copy_from_slice(b"cmap");
    repair[20..24].copy_from_slice(&44u32.to_be_bytes());
    repair[24..28].copy_from_slice(&26u32.to_be_bytes());
    repair[28..32].copy_from_slice(b"head");
    repair[36..40].copy_from_slice(&72u32.to_be_bytes());
    repair[40..44].copy_from_slice(&12u32.to_be_bytes());
    repair[46..48].copy_from_slice(&1u16.to_be_bytes());
    repair[48..50].copy_from_slice(&1u16.to_be_bytes());
    repair[52..56].copy_from_slice(&12u32.to_be_bytes());
    for (offset, value) in [(56, 6u16), (58, 14), (62, 1), (64, 2), (66, 7), (68, 9)] {
        repair[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    let fonts = build(
        &target,
        source.as_bytes(),
        |number| {
            Some(if number == 101 {
                font(0)
            } else {
                repair.clone()
            })
        },
        |_| cmap.clone(),
    );
    assert!(fonts.is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            ..
        })
    ));
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 任意のCMap取得位置で停止しても、取得を続けず部分結果を公開しない。
    #[test]
    fn stopped_mapping_never_publishes_partial_output(count in 1usize..33, stop in 0usize..32) {
        let stop = stop % count;
        let names: Vec<_> = (0..count).map(|number| format!("F{number:02}")).collect();
        let source = descriptors(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let target = budget(100000);
        let calls = Cell::new(0);
        let fonts = build(&target, source.as_bytes(), |_| Some(font(0)), |_| {
            let position = calls.get();
            calls.set(position + 1);
            if position == stop { assert!(target.charge_metadata_workspace_bytes(100001).is_none()); }
            Default::default()
        });
        prop_assert!(fonts.is_empty());
        prop_assert_eq!(calls.get(), stop + 1);
        assert_limit(&target);
    }
    /// 全name復号・候補の成長・候補全件の出力slotと再作成の独立モデルを検査する。
    #[test]
    fn capacity_matches_model(
        aliases in prop::collection::vec(0usize..16, 0..40),
        builds in 1usize..4,
        short in any::<bool>(),
    ) {
        let names: Vec<_> = aliases.iter().map(|alias| format!("F{alias:02}#c3#a9")).collect();
        let source = descriptors(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let expected: std::collections::BTreeSet<_> = aliases.iter().copied().collect();
        let count = expected.len();
        let one = aliases.len() * 5 + slots(count) + results(count);
        let capacity = (one * builds).saturating_sub(usize::from(short));
        let target = budget(capacity);
        for pass in 0..builds {
            let fonts = build(&target, source.as_bytes(), |_| Some(font(0)), |_| Default::default());
            if (pass + 1) * one <= capacity {
                let actual: Vec<_> = fonts.iter().map(|font| font.font_name.as_str()).collect();
                let expected: Vec<_> = expected.iter().map(|alias| format!("F{alias:02}é")).collect();
                prop_assert_eq!(actual, expected);
            } else { prop_assert!(fonts.is_empty()); }
        }
        prop_assert_eq!(target.check().is_ok(), one * builds <= capacity);
    }
}
