use super::*;
use crate::{ParseError, ParseLimits};

/// 二文字の format 6 と checksum adjustment を持つ最小の修復対象を作る。
fn subset_font() -> Vec<u8> {
    let mut bytes = vec![0; 84];
    bytes[..4].copy_from_slice(&[0, 1, 0, 0]);
    bytes[4..6].copy_from_slice(&2u16.to_be_bytes());
    bytes[12..16].copy_from_slice(b"cmap");
    bytes[20..24].copy_from_slice(&44u32.to_be_bytes());
    bytes[24..28].copy_from_slice(&26u32.to_be_bytes());
    bytes[28..32].copy_from_slice(b"head");
    bytes[36..40].copy_from_slice(&72u32.to_be_bytes());
    bytes[40..44].copy_from_slice(&12u32.to_be_bytes());
    bytes[46..48].copy_from_slice(&1u16.to_be_bytes());
    bytes[48..50].copy_from_slice(&1u16.to_be_bytes());
    bytes[52..56].copy_from_slice(&12u32.to_be_bytes());
    for (offset, value) in [(56, 6u16), (58, 14), (62, 1), (64, 2), (66, 7), (68, 9)] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    bytes[80..84].copy_from_slice(&123u32.to_be_bytes());
    bytes
}

/// 修復する文字対応が正しくても、保存予算ゼロなら確保前に型付きエラーを残す。
#[test]
fn repair_stops_before_allocating_when_font_budget_is_zero() {
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(
        repair_sfnt_unicode_cmap(&budget, &subset_font(), [(1, 65), (2, 66)].into_iter()).is_none()
    );
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "embedded font bytes", limit: 0, actual
    }) if actual > 0));
    assert!(budget.is_stopped());
}

/// 有効な既存修復は全体 checksum と異なる glyph の対応を保持する。
#[test]
fn repair_preserves_glyphs_and_sfnt_checksum() {
    let font = subset_font();
    let repaired = repair_sfnt_unicode_cmap(
        &DecodeBudget::default(),
        &font,
        [(1, 65), (2, 66)].into_iter(),
    )
    .unwrap();
    assert_eq!(sfnt_checksum(&repaired), 0xb1b0_afba);
    let cmap = sfnt_table(&repaired, b"cmap").unwrap();
    assert_eq!(read_be_u16(cmap, 20), Some(4));
    assert_eq!(read_be_u16(cmap, 48), Some(7u16.wrapping_sub(65)));
    assert_eq!(read_be_u16(cmap, 50), Some(9u16.wrapping_sub(66)));
    assert_eq!(font[80..84], 123u32.to_be_bytes());
}

/// 最後の要求量へ上限を進めて、修復全体の累積境界を調べる。
#[test]
fn repair_allocation_boundary_is_inclusive_and_shared() {
    let font = subset_font();
    let mut capacity = 0;
    let mut complete = false;
    for _ in 0..8 {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: capacity,
            ..Default::default()
        });
        let repaired = repair_sfnt_unicode_cmap(&budget, &font, [(1, 65), (2, 66)].into_iter());
        match budget.check() {
            Err(ParseError::ResourceLimit {
                resource: "embedded font bytes",
                limit,
                actual,
            }) => {
                assert!(repaired.is_none());
                assert_eq!(limit, capacity);
                assert!(actual > capacity);
                capacity = actual;
            }
            Ok(()) => {
                assert!(repaired.is_some());
                complete = true;
                break;
            }
            result => panic!("unexpected {result:?}"),
        }
    }
    assert!(complete);
    assert!(capacity > font.len() as u64);
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: capacity - 1,
        ..Default::default()
    });
    assert!(repair_sfnt_unicode_cmap(&budget, &font, [(1, 65), (2, 66)].into_iter()).is_none());
    assert!(
        matches!(budget.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == capacity)
    );
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: capacity,
        ..Default::default()
    });
    assert!(repair_sfnt_unicode_cmap(&budget, &font, [(1, 65), (2, 66)].into_iter()).is_some());
    assert!(repair_sfnt_unicode_cmap(&budget, &font, [(1, 65), (2, 66)].into_iter()).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit { .. })
    ));
    assert!(repair_sfnt_unicode_cmap(&budget, &font, [(1, 65)].into_iter()).is_none());
}

/// 修復後の format 4 から、単一 code segment の glyph を独立に読む。
fn repaired_glyphs(bytes: &[u8]) -> std::collections::BTreeMap<u16, u16> {
    let cmap = sfnt_table(bytes, b"cmap").unwrap();
    let count = usize::from(read_be_u16(cmap, 26).unwrap()) / 2;
    (0..count - 1)
        .map(|index| {
            let end = read_be_u16(cmap, 34 + 2 * index).unwrap();
            let start = read_be_u16(cmap, 36 + 2 * count + 2 * index).unwrap();
            assert_eq!(start, end);
            let delta = read_be_u16(cmap, 36 + 4 * count + 2 * index).unwrap();
            (start, start.wrapping_add(delta))
        })
        .collect()
}

/// Unicode の重複は最初の source を保持し、元配列の範囲外 source は修復しない。
#[test]
fn repair_keeps_first_unicode_mapping_and_rejects_missing_source() {
    let font = subset_font();
    let repaired = repair_sfnt_unicode_cmap(
        &DecodeBudget::default(),
        &font,
        [(2, 65), (1, 65), (1, 66)].into_iter(),
    )
    .unwrap();
    assert_eq!(
        repaired_glyphs(&repaired),
        [(65, 9), (66, 7)].into_iter().collect()
    );
    assert!(
        repair_sfnt_unicode_cmap(
            &DecodeBudget::default(),
            &font,
            [(1, 65), (3, 66)].into_iter()
        )
        .is_none()
    );
    let mut missing = font.clone();
    missing[66..68].fill(0);
    let repaired = repair_sfnt_unicode_cmap(
        &DecodeBudget::default(),
        &missing,
        [(1, 65), (2, 66)].into_iter(),
    )
    .unwrap();
    assert_eq!(repaired_glyphs(&repaired), [(66, 9)].into_iter().collect());
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 入力順の独立 map と比較し、重複対応・整列・checksum の不変条件を固定する。
    #[test]
    fn repair_matches_first_assignment_model(mappings in proptest::collection::vec((1u16..=2, 0u16..0xd800), 1..64)) {
        let mut expected = std::collections::BTreeMap::new();
        for &(source, unicode) in &mappings { expected.entry(unicode).or_insert(if source == 1 { 7 } else { 9 }); }
        let repaired = repair_sfnt_unicode_cmap(&DecodeBudget::default(), &subset_font(), mappings.iter().copied()).unwrap();
        prop_assert_eq!(repaired_glyphs(&repaired), expected);
        prop_assert_eq!(sfnt_checksum(&repaired), 0xb1b0_afba);
    }
}
