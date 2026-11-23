//! 自前の読取式で、format4/12の全mappingと未対応値を検証する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 大端16bitを読む。
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}
/// 大端32bitを読む。
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}
/// Windowsの指定encoding recordを探して借用する。
fn subtable(bytes: &[u8], encoding: u16) -> &[u8] {
    for index in 0..u16_at(bytes, 2) as usize {
        let row = 4 + index * 8;
        if u16_at(bytes, row) == 3 && u16_at(bytes, row + 2) == encoding {
            return &bytes[u32_at(bytes, row + 4) as usize..];
        }
    }
    panic!("missing Windows cmap encoding {encoding}")
}
/// format4の境界配列とdeltaから、元のGIDを独立に復元する。
fn lookup4(bytes: &[u8], scalar: u16) -> u16 {
    let bytes = subtable(bytes, 1);
    assert_eq!(u16_at(bytes, 0), 4);
    let count = u16_at(bytes, 6) as usize / 2;
    for index in 0..count {
        if scalar >= u16_at(bytes, 16 + 2 * count + 2 * index)
            && scalar <= u16_at(bytes, 14 + 2 * index)
        {
            assert_eq!(u16_at(bytes, 16 + 6 * count + 2 * index), 0);
            return scalar.wrapping_add(u16_at(bytes, 16 + 4 * count + 2 * index));
        }
    }
    0
}
/// format12の32bit範囲から、元のGIDを独立に復元する。
fn lookup12(bytes: &[u8], scalar: u32) -> u16 {
    let bytes = subtable(bytes, 10);
    assert_eq!(u16_at(bytes, 0), 12);
    for index in 0..u32_at(bytes, 12) as usize {
        let row = 16 + 12 * index;
        let start = u32_at(bytes, row);
        if scalar >= start && scalar <= u32_at(bytes, row + 4) {
            return (u32_at(bytes, row + 8) + scalar - start) as u16;
        }
    }
    0
}

/// BMP・補助平面・非文字scalarの対応と、未対応値の.notdefを区別する。
#[test]
fn bmp_and_full_unicode_tables_preserve_source_glyph_ids() {
    let mappings = [
        Mapping {
            scalar: 0x10400,
            glyph: 4,
        },
        Mapping {
            scalar: 65,
            glyph: 1,
        },
        Mapping {
            scalar: 66,
            glyph: 2,
        },
        Mapping {
            scalar: 233,
            glyph: 3,
        },
        Mapping {
            scalar: 0x10ffff,
            glyph: 5,
        },
        Mapping {
            scalar: 0xffff,
            glyph: 6,
        },
    ];
    let bytes = encode(&DecodeBudget::default(), &mappings, 7).unwrap();
    assert_eq!(u16_at(&bytes, 0), 0);
    assert_eq!(u16_at(&bytes, 2), 4);
    for mapping in mappings {
        assert_eq!(lookup12(&bytes, mapping.scalar), mapping.glyph);
        if mapping.scalar < 0xffff {
            assert_eq!(lookup4(&bytes, mapping.scalar as u16), mapping.glyph);
        }
    }
    assert_eq!(lookup4(&bytes, 0xffff), 0);
    assert_eq!(lookup12(&bytes, 0), 0);
    assert_eq!(lookup12(&bytes, 67), 0);
    assert_eq!(u32_at(&bytes, 8), u32_at(&bytes, 24));
    assert_eq!(u32_at(&bytes, 16), u32_at(&bytes, 32));
}

/// 同一対応の重複は除けるが、異なる外形を一つのUnicodeへ推測で統合しない。
#[test]
fn duplicate_identity_invalid_scalar_and_missing_glyph_are_not_guessed() {
    let one = [Mapping {
        scalar: 65,
        glyph: 1,
    }];
    let repeated = [one[0], one[0]];
    assert_eq!(
        encode(&DecodeBudget::default(), &one, 3),
        encode(&DecodeBudget::default(), &repeated, 3)
    );
    for mappings in [
        vec![
            Mapping {
                scalar: 65,
                glyph: 1,
            },
            Mapping {
                scalar: 65,
                glyph: 2,
            },
        ],
        vec![Mapping {
            scalar: 65,
            glyph: 0,
        }],
        vec![Mapping {
            scalar: 65,
            glyph: 3,
        }],
        vec![Mapping {
            scalar: 0xd800,
            glyph: 1,
        }],
        vec![Mapping {
            scalar: 0x110000,
            glyph: 1,
        }],
    ] {
        assert!(encode(&DecodeBudget::default(), &mappings, 3).is_none());
    }
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(encode(&budget, &one, 3).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            ..
        })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意Unicodeの対応が、入力順に依存せず両形式から再現できる。
    #[test]
    fn unicode_mapping_is_order_invariant(items in proptest::collection::vec((any::<char>(),1_u16..64),0..80),reverse in any::<bool>()) {
        let wanted=items.into_iter().collect::<std::collections::BTreeMap<_,_>>();
        let mut mappings=wanted.iter().map(|(&ch,&glyph)|Mapping{scalar:u32::from(ch),glyph}).collect::<Vec<_>>();
        let forward=encode(&DecodeBudget::default(),&mappings,64).unwrap();
        if reverse { mappings.reverse(); }
        let before=mappings.clone();let encoded=encode(&DecodeBudget::default(),&mappings,64).unwrap();
        prop_assert_eq!(encoded.as_slice(),forward.as_slice());prop_assert_eq!(mappings,before);
        for (&ch,&glyph) in &wanted { prop_assert_eq!(lookup12(&encoded,u32::from(ch)),glyph);if u32::from(ch)<0xffff { prop_assert_eq!(lookup4(&encoded,ch as u16),glyph); } }
    }
}
