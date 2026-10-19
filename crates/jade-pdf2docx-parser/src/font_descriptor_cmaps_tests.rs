//! FontDescriptor対応mapの保存容量と共有payloadを独立に検査する。
use super::*;
use crate::{ParseError, ParseLimits};

/// Descriptor保存だけのmetadata予算を作る。
fn budget(limit: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit as u64,
        ..Default::default()
    })
}

/// 完成済みのBMP対応を別予算で用意する。
fn payload() -> ToUnicodeCMap {
    let cmap = crate::text::parse_tounicode_body(
        &DecodeBudget::default(),
        b"1 beginbfchar\n<41> <0041>\nendbfchar",
    );
    assert!(!cmap.is_empty());
    cmap
}

/// 初回の全slot要求容量を独立に求める。
fn one() -> usize {
    4 * std::mem::size_of::<Option<(u32, ToUnicodeCMap)>>()
}

/// metadata容量超過が公開エラーとして残る。
fn assert_limit(target: &DecodeBudget) {
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 完成payloadを共有するだけでもDescriptor map slotの予算は必要。
#[test]
fn zero_capacity_rejects_descriptor_map() {
    let target = budget(0);
    let cmap = payload();
    let maps = build(
        &target,
        b"1 0 obj << /Type /Font /FontDescriptor 9 0 R >> endobj",
        |_| Some(&cmap),
    );
    assert!(maps.is_empty());
    assert_limit(&target);
}

/// 解放後に同じDescriptor対応を作っても要求容量は返却しない。
#[test]
fn descriptor_map_recreation_is_cumulative() {
    let target = budget(2 * one() - 1);
    let cmap = payload();
    let source = b"1 0 obj << /Type /Font /FontDescriptor 9 0 R >> endobj";
    assert!(!build(&target, source, |_| Some(&cmap)).is_empty());
    assert!(build(&target, source, |_| Some(&cmap)).is_empty());
    assert_limit(&target);
}

/// 3件目の拡張が失敗した場合、先に完成したDescriptor entryも公開しない。
#[test]
fn descriptor_growth_failure_discards_completed_entries() {
    let target = budget(one());
    let cmap = payload();
    let source = b"1 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj\n2 0 obj << /Type /Font /FontDescriptor 8 0 R >> endobj\n3 0 obj << /Type /Font /FontDescriptor 9 0 R >> endobj";
    assert!(build(&target, source, |_| Some(&cmap)).is_empty());
    assert_limit(&target);
}

/// 完成CMap取得中に予算が止まっても部分mapを保存しない。
#[test]
fn stopped_lookup_discards_descriptor_map() {
    let target = budget(one());
    let cmap = payload();
    let source = b"1 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj\n2 0 obj << /Type /Font /FontDescriptor 8 0 R >> endobj";
    let maps = build(&target, source, |number| {
        if number == 2 {
            assert!(
                target
                    .charge_metadata_workspace_bytes(one() as u64 + 1)
                    .is_none()
            );
        }
        Some(&cmap)
    });
    assert!(maps.is_empty());
    assert_limit(&target);
}

/// 正確なslot境界では共有payloadだけを保存し、別名でも再解析しない。
#[test]
fn exact_descriptor_capacity_shares_cmap_without_payload_budget() {
    let cmap = payload();
    let target = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: one() as u64,
        max_total_tounicode_bytes: 0,
        max_total_tounicode_storage_bytes: 0,
        ..Default::default()
    });
    let source = b"1 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj\n2 0 obj << /Type /Font /FontDescriptor 8 0 R >> endobj";
    let maps = build(&target, source, |_| Some(&cmap));
    for number in [7, 8] {
        assert!(maps.get(&number).unwrap().shares_storage_with(&cmap));
    }
    target.check().unwrap();
    let target = budget(one() - 1);
    assert!(build(&target, source, |_| Some(&cmap)).is_empty());
    assert_limit(&target);
}

/// 同じDescriptorはobject番号順でなく、入力順の最初のBMP対応を選ぶ。
#[test]
fn duplicate_descriptor_keeps_first_input_mapping() {
    let first = payload();
    let second = crate::text::parse_tounicode_body(
        &DecodeBudget::default(),
        b"1 beginbfchar\n<41> <0042>\nendbfchar",
    );
    let target = budget(one());
    let source = b"9 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj\n1 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj";
    let maps = build(&target, source, |number| {
        Some(if number == 9 { &first } else { &second })
    });
    assert!(maps.get(&7).unwrap().shares_storage_with(&first));
    target.check().unwrap();
}

/// BMP対応がないCMapとCMap未取得のFontはslotを消費せず後続の候補を採用する。
#[test]
fn invalid_candidates_do_not_reserve_descriptor_slots() {
    let empty = ToUnicodeCMap::default();
    let non_bmp = crate::text::parse_tounicode_body(
        &DecodeBudget::default(),
        b"1 beginbfchar\n<41> <D83DDE00>\nendbfchar",
    );
    assert!(!non_bmp.is_empty());
    let good = payload();
    let source = b"1 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj\n2 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj\n3 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj\n4 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj";
    let target = budget(one());
    let maps = build(&target, source, |number| match number {
        1 => None,
        2 => Some(&empty),
        3 => Some(&non_bmp),
        _ => Some(&good),
    });
    assert!(maps.get(&7).unwrap().shares_storage_with(&good));
    target.check().unwrap();
    let target = budget(0);
    assert!(build(&target, source, |_| Some(&non_bmp)).is_empty());
    target.check().unwrap();
}

/// エラーを消費した後も停止したmap構築はCMap取得を開始しない。
#[test]
fn stopped_descriptor_map_does_not_call_lookup() {
    let target = budget(0);
    assert!(target.charge_metadata_workspace_bytes(1).is_none());
    assert_limit(&target);
    let source = b"1 0 obj << /Type /Font /FontDescriptor 7 0 R >> endobj";
    assert!(build(&target, source, |_| panic!("stopped lookup")).is_empty());
}

/// 空入力やFont以外の辞書だけならDescriptor slotを作らない。
#[test]
fn irrelevant_objects_need_no_descriptor_map_storage() {
    for source in [
        b"".as_slice(),
        b"1 0 obj << /Type /Other /FontDescriptor 7 0 R >> endobj",
        b"1 0 obj << /Type /Font /Note << /FontDescriptor 7 0 R >> >> endobj",
    ] {
        let target = budget(0);
        assert!(build(&target, source, |_| panic!("irrelevant lookup")).is_empty());
        target.check().unwrap();
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 独立した件数/slotモデルで重複・初出payload・境界・再作成を検査する。
    #[test]
    fn descriptor_history_matches_first_mapping_capacity_model(aliases in prop::collection::vec(0usize..20, 1..64)) {
        let first = payload();
        let second = crate::text::parse_tounicode_body(&DecodeBudget::default(), b"1 beginbfchar\n<41> <0042>\nendbfchar");
        let mut source = String::new();
        let mut chosen = [None; 20];
        let mut count = 0;
        let mut capacity = 0;
        let mut bytes = 0;
        for (index, &alias) in aliases.iter().enumerate() {
            source.push_str(&format!("{} 0 obj << /Type /Font /FontDescriptor {} 0 R >> endobj\n", index + 1, alias + 100));
            if chosen[alias].is_none() {
                if count >= capacity / 2 {
                    capacity = (capacity * 2).max(4);
                    bytes += capacity * std::mem::size_of::<Option<(u32, ToUnicodeCMap)>>();
                }
                chosen[alias] = Some((index + 1) % 2 == 0);
                count += 1;
            }
        }
        let lookup = |number| Some(if number % 2 == 0 { &first } else { &second });
        let target = budget(bytes - 1);
        prop_assert!(build(&target, source.as_bytes(), lookup).is_empty());
        assert_limit(&target);
        let target = budget(2 * bytes - 1);
        let maps = build(&target, source.as_bytes(), lookup);
        for (alias, choice) in chosen.iter().enumerate() {
            if let Some(choice) = choice {
                let expected = if *choice { &first } else { &second };
                prop_assert!(maps.get(&(alias as u32 + 100)).unwrap().shares_storage_with(expected));
            }
        }
        drop(maps);
        prop_assert!(build(&target, source.as_bytes(), lookup).is_empty());
        assert_limit(&target);
        let target = budget(2 * bytes);
        for _ in 0..2 { prop_assert!(!build(&target, source.as_bytes(), lookup).is_empty()); }
        target.check().unwrap();
    }

    /// 任意の取得位置で停止すると部分mapを捨て、その後の取得も開始しない。
    #[test]
    fn stopped_lookup_history_never_publishes_partial_descriptors(aliases in prop::collection::vec(0usize..20, 1..64), seed in any::<usize>()) {
        let cmap = payload();
        let limit = one() * 256;
        let target = budget(limit);
        let stop = seed % aliases.len() + 1;
        let mut source = String::new();
        for (index, alias) in aliases.iter().enumerate() {
            source.push_str(&format!("{} 0 obj << /Type /Font /FontDescriptor {} 0 R >> endobj\n", index + 1, alias + 100));
        }
        let calls = std::cell::Cell::new(0usize);
        let maps = build(&target, source.as_bytes(), |number| {
            assert!(number as usize <= stop);
            calls.set(calls.get() + 1);
            if number as usize == stop { assert!(target.charge_metadata_workspace_bytes(limit as u64 + 1).is_none()); }
            Some(&cmap)
        });
        prop_assert!(maps.is_empty());
        prop_assert_eq!(calls.get(), stop);
        assert_limit(&target);
        prop_assert!(build(&target, source.as_bytes(), |_| panic!("consumed stop")).is_empty());
    }
}
