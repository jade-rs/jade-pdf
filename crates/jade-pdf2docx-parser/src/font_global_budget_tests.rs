//! 全体Font CMap索引の要求容量を検査する。
use super::*;

/// 非Fontの候補slotも索引容量として確保前に検査する。
#[test]
fn zero_capacity_rejects_font_cmap_index_candidates() {
    let target = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: 0,
        ..Default::default()
    });
    let objects =
        crate::page_attributes::Objects::from([(1, (0, b"<< /Type /Other >>".as_slice()))]);
    assert!(
        font_object_tounicode_cmaps_in(&target, b"", &objects, &StreamOffsets::default())
            .is_empty()
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// Font以外を含む全候補slotを予約し、解放後の再作成を数える。
#[test]
fn font_cmap_index_exact_capacity_and_recreation() {
    let objects = crate::page_attributes::Objects::from([
        (1, (0, b"<< /Type /Other >>".as_slice())),
        (9, (0, b"<< /Type /Font >>".as_slice())),
    ]);
    let one = 2 * std::mem::size_of::<(u32, u16, ToUnicodeCMap)>();
    for (limit, builds) in [(one - 1, 1), (one, 1), (2 * one - 1, 2), (2 * one, 2)] {
        let target = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: limit as u64,
            ..Default::default()
        });
        for _ in 0..builds {
            assert!(
                font_object_tounicode_cmaps_in(&target, b"", &objects, &StreamOffsets::default())
                    .is_empty()
            );
        }
        assert_eq!(target.check().is_ok(), limit >= builds * one);
    }
}

/// 後続CMapの解析が止まった場合、先に完成したCMapも部分索引へ返さない。
#[test]
fn stopped_cmap_payload_discards_entire_font_index() {
    let objects = crate::page_attributes::Objects::from([
        (1, (0, b"<< /Type /Font /ToUnicode 3 0 R >>".as_slice())),
        (2, (0, b"<< /Type /Font /ToUnicode 4 0 R >>".as_slice())),
        (3, (0, b"<< >>".as_slice())),
        (4, (0, b"<< >>".as_slice())),
    ]);
    let first = "1 beginbfchar\n<41> <0041>\nendbfchar";
    let second = "1 beginbfchar\n<42> <0042>\nendbfchar";
    let source = format!(
        "3 0 obj << /Length {} >>\nstream\n{first}\nendstream\nendobj\n4 0 obj << /Length {} >>\nstream\n{second}\nendstream\nendobj",
        first.len(),
        second.len()
    );
    let source = source.as_bytes();
    let success = font_object_tounicode_cmaps_in(
        &DecodeBudget::default(),
        source,
        &objects,
        &StreamOffsets::default(),
    );
    assert!(success.get(&1).is_some());
    assert!(success.get(&2).is_some());
    assert_eq!(
        success
            .into_iter()
            .map(|(number, _, _)| number)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let target = DecodeBudget::new(&ParseLimits {
        max_total_tounicode_entries: 1,
        ..Default::default()
    });
    let maps = font_object_tounicode_cmaps_in(&target, source, &objects, &StreamOffsets::default());
    assert!(maps.is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit { .. })
    ));
    assert!(
        font_object_tounicode_cmaps_in(&target, source, &objects, &StreamOffsets::default())
            .is_empty()
    );
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 索引の全候補件数と再作成回数の独立容量モデルを検査する。
    #[test]
    fn font_cmap_index_matches_candidate_capacity_model(count in 1usize..64, builds in 1usize..8, short in any::<bool>()) {
        let objects: crate::page_attributes::Objects<'_> = (0..count).map(|number| (number as u32, (0, b"<< /Type /Other >>".as_slice()))).collect();
        let bytes = count * builds * std::mem::size_of::<(u32, u16, ToUnicodeCMap)>();
        let target = DecodeBudget::new(&ParseLimits { max_total_metadata_workspace_bytes: (bytes - usize::from(short)) as u64, ..Default::default() });
        for _ in 0..builds { prop_assert!(font_object_tounicode_cmaps_in(&target, b"", &objects, &StreamOffsets::default()).is_empty()); }
        prop_assert_eq!(target.check().is_ok(), !short);
    }
}

/// 全体BaseFont/Encoding/CMap経路は非0世代の有効Fontだけへ名前を対応付ける。
#[test]
fn fallback_maps_keep_nonzero_font_generations() {
    let body = "1 beginbfchar\n<41> <0041>\nendbfchar";
    let source = format!(
        "1 3 obj << /Type /Font /BaseFont /Good /Encoding /Identity-H /ToUnicode 2 0 R >> endobj\n2 0 obj << /Length {} >>\nstream\n{body}\nendstream\nendobj\n3 0 obj << /Resources << /Font << /Valid 1 3 R /Stale 1 0 R /Wrong 11 3 R >> >> >> endobj",
        body.len()
    );
    let target = DecodeBudget::default();
    let base = parse_font_resource_base_fonts(&target, source.as_bytes());
    let encoding = parse_font_resource_encodings(&target, source.as_bytes());
    let cmaps = parse_font_resource_tounicode_cmaps(&target, source.as_bytes());
    assert_eq!(base.get("Valid").map(String::as_str), Some("Good"));
    assert_eq!(
        encoding.get("Valid").map(String::as_str),
        Some("Identity-H")
    );
    assert!(!cmaps.get("Valid").unwrap().is_empty());
    for name in ["Stale", "Wrong"] {
        assert!(base.get(name).is_none());
        assert!(encoding.get(name).is_none());
        assert!(cmaps.get(name).is_none());
    }
    target.check().unwrap();
}
