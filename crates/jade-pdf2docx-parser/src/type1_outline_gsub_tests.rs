//! GSUBのoffsetを辿り、論理列・長い合字の優先・大きなsubtableを独立に読む。
use super::*;
use crate::{ParseCancellation, ParseError, ParseLimits};
use proptest::prelude::*;

/// 検査側のbig-endian reader。境界外ならテストを失敗させる。
fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}

/// 固定アドレスではなく、表の参照関係から全合字とrequired featureを読む。
fn read(bytes: &[u8]) -> Vec<(Vec<u16>, u16)> {
    if bytes.is_empty() {
        return Vec::new();
    }
    assert_eq!(&bytes[..4], &[0, 1, 0, 0]);
    let scripts = usize::from(word(bytes, 4));
    assert_eq!(word(bytes, scripts), 1);
    assert_eq!(&bytes[scripts + 2..scripts + 6], b"DFLT");
    let script = scripts + usize::from(word(bytes, scripts + 6));
    let language = script + usize::from(word(bytes, script));
    assert_eq!(word(bytes, language), 0);
    assert_eq!(word(bytes, language + 2), 0);
    let features = usize::from(word(bytes, 6));
    assert_eq!(word(bytes, features), 1);
    assert_eq!(&bytes[features + 2..features + 6], b"rlig");
    let feature = features + usize::from(word(bytes, features + 6));
    assert_eq!(word(bytes, feature + 2), 1);
    assert_eq!(word(bytes, feature + 4), 0);
    let lookups = usize::from(word(bytes, 8));
    assert_eq!(word(bytes, lookups), 1);
    let lookup = lookups + usize::from(word(bytes, lookups + 2));
    assert_eq!(word(bytes, lookup), 7);
    assert_eq!(word(bytes, lookup + 2), 0);
    (0..usize::from(word(bytes, lookup + 4)))
        .map(|i| {
            let extension = lookup + usize::from(word(bytes, lookup + 6 + 2 * i));
            assert_eq!(word(bytes, extension), 1);
            assert_eq!(word(bytes, extension + 2), 4);
            let offset = u32::from_be_bytes(bytes[extension + 4..extension + 8].try_into().unwrap())
                as usize;
            let subtable = extension + offset;
            assert_eq!(word(bytes, subtable), 1);
            assert_eq!(word(bytes, subtable + 4), 1);
            let coverage = subtable + usize::from(word(bytes, subtable + 2));
            assert_eq!(word(bytes, coverage), 1);
            assert_eq!(word(bytes, coverage + 2), 1);
            let set = subtable + usize::from(word(bytes, subtable + 6));
            assert_eq!(word(bytes, set), 1);
            let ligature = set + usize::from(word(bytes, set + 2));
            let mut components = vec![word(bytes, coverage + 4)];
            for c in 1..usize::from(word(bytes, ligature + 2)) {
                components.push(word(bytes, ligature + 2 + 2 * c));
            }
            (components, word(bytes, ligature))
        })
        .collect()
}

/// 原文のffiをffへ先取りせず、元のcomponent列と出力glyphを保つ。
#[test]
fn longer_ligatures_precede_prefixes_in_required_feature() {
    let rules = [
        Rule {
            components: &[1, 1],
            glyph: 5,
        },
        Rule {
            components: &[1, 1, 2],
            glyph: 6,
        },
        Rule {
            components: &[1, 2],
            glyph: 7,
        },
    ];
    let bytes = encode(&DecodeBudget::default(), &rules, 8).unwrap();
    let decoded = read(&bytes);
    assert_eq!(
        decoded,
        [(vec![1, 1, 2], 6), (vec![1, 1], 5), (vec![1, 2], 7)]
    );
    let input = [1, 1, 2, 1, 1, 1, 2];
    let mut cursor = 0;
    let mut output = Vec::new();
    while cursor < input.len() {
        if let Some((sequence, glyph)) =
            decoded.iter().find(|(s, _)| input[cursor..].starts_with(s))
        {
            output.push(*glyph);
            cursor += sequence.len();
        } else {
            output.push(input[cursor]);
            cursor += 1;
        }
    }
    assert_eq!(output, [6, 5, 7]);
}

/// 内部Ligature配列が64KiBを超えても、後続のsubtableを切り詰めない。
#[test]
fn extension_offsets_preserve_long_component_sequences() {
    let long = vec![1; 40_000];
    let rules = [
        Rule {
            components: &long,
            glyph: 4,
        },
        Rule {
            components: &[2, 3],
            glyph: 5,
        },
    ];
    let bytes = encode(&DecodeBudget::default(), &rules, 6).unwrap();
    assert!(bytes.len() > 65_535);
    assert_eq!(read(&bytes), [(long, 4), (vec![2, 3], 5)]);
    let list = usize::from(word(&bytes, 8));
    let lookup = list + usize::from(word(&bytes, list + 2));
    let second = lookup + usize::from(word(&bytes, lookup + 8));
    assert!(u32::from_be_bytes(bytes[second + 4..second + 8].try_into().unwrap()) > 65_535);
}

/// 同じ列の別glyphを勝手に選ばず、不明GIDや一文字を合字として公開しない。
#[test]
fn ambiguous_or_invalid_glyph_rules_are_rejected_atomically() {
    let conflict = [
        Rule {
            components: &[1, 2],
            glyph: 3,
        },
        Rule {
            components: &[1, 2],
            glyph: 4,
        },
    ];
    assert!(encode(&DecodeBudget::default(), &conflict, 5).is_none());
    for rule in [
        Rule {
            components: &[],
            glyph: 3,
        },
        Rule {
            components: &[1],
            glyph: 3,
        },
        Rule {
            components: &[0, 1],
            glyph: 3,
        },
        Rule {
            components: &[1, 5],
            glyph: 3,
        },
        Rule {
            components: &[1, 2],
            glyph: 0,
        },
        Rule {
            components: &[1, 2],
            glyph: 5,
        },
    ] {
        assert!(encode(&DecodeBudget::default(), &[rule], 5).is_none());
    }
    let too_long = vec![1; 65_536];
    assert!(
        encode(
            &DecodeBudget::default(),
            &[Rule {
                components: &too_long,
                glyph: 3
            }],
            5
        )
        .is_none()
    );
}

/// 空の合字表はheapを使わず、停止した予算では空表も成功として返さない。
#[test]
fn empty_rules_and_cancelled_budgets_remain_distinct() {
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert_eq!(encode(&budget, &[], 2), Some(Vec::new()));
    assert!(budget.check().is_ok());
    let token = ParseCancellation::new();
    token.cancel();
    let stopped = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(encode(&stopped, &[], 2).is_none());
    assert!(matches!(stopped.check(), Err(ParseError::Cancelled)));
}

/// 借用規則配列と完成86byteを正確に予約し、一byte不足時には全表を破棄する。
#[test]
fn exact_storage_budget_includes_ordering_and_finished_table() {
    let rules = [Rule {
        components: &[1, 2],
        glyph: 3,
    }];
    let size = 86 + std::mem::size_of::<&Rule<'_>>() as u64;
    for limit in [size, size - 1] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: limit,
            ..Default::default()
        });
        let result = encode(&budget, &rules, 4);
        if limit == size {
            assert_eq!(result.as_ref().map(Vec::len), Some(86));
            assert!(budget.check().is_ok());
        } else {
            assert!(result.is_none());
            assert!(
                matches!(budget.check(),Err(ParseError::ResourceLimit {resource:"embedded font bytes",limit:l,actual})if l==limit && actual==size)
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意の列集合を途中で短縮せず、入力順と重複に依らず同じ決定的表へ格納する。
    #[test]
    fn arbitrary_rules_keep_complete_sequences_and_canonical_order(
        sequences in prop::collection::vec(prop::collection::vec(1u16..8,2..12),0..32)
    ) {
        let rules=sequences.iter().map(|s|Rule {components:s,glyph:9}).collect::<Vec<_>>();
        let expected=sequences.iter().cloned().map(|s|(s,9)).collect::<std::collections::BTreeSet<_>>();
        let budget=DecodeBudget::default();let bytes=encode(&budget,&rules,10).unwrap();
        prop_assert_eq!(read(&bytes).into_iter().collect::<std::collections::BTreeSet<_>>(),expected);
        let reverse=rules.into_iter().rev().collect::<Vec<_>>();
        prop_assert_eq!(encode(&budget,&reverse,10),Some(bytes));
        prop_assert!(budget.check().is_ok());
    }
}
