//! 検査済みstream索引の確保と再作成を共有metadata予算で制限する。
use super::*;
use crate::{ParseError, ParseLimits, decode::DecodeBudget};

/// xrefのない入力から構造検査済みstream一覧を借用する。
fn spans(bytes: &[u8]) -> Vec<StreamSpan<'_>> {
    crate::structure::validate_structure(
        std::path::Path::new("stream-index.pdf"),
        bytes,
        8,
        &ParseLimits::default(),
        &DecodeBudget::default(),
    )
    .unwrap()
}

/// metadata容量以外は既定値のままにする。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 索引の保存は容量0で拒否し、元入力を変更しない。
#[test]
fn stream_index_cannot_be_cached_without_metadata_capacity() {
    let bytes = b"%PDF-1.7\n1 0 obj << /Length 3 >> stream\nabc\nendstream endobj";
    let streams = spans(bytes);
    let budget = budget(0);
    budget.cache_streams(bytes, &streams);
    assert!(budget.stream_index(bytes).is_none());
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: 0, actual,
    }) if actual > 0));
    assert_eq!(streams[0].payload, b"abc");
}

/// rangeと番号/offset配列の要求容量をちょうど一回分だけ許可する。
#[test]
fn stream_index_capacity_is_cumulative_and_inclusive() {
    let bytes = b"%PDF-1.7\n1 0 obj << /Length 3 >> stream\nabc\nendstream endobj";
    let streams = spans(bytes);
    let one = (std::mem::size_of::<StreamRange>()
        + std::mem::size_of::<(u32, usize)>()
        + std::mem::size_of::<(usize, usize)>()) as u64;
    for limit in [one - 1, one, one * 2 - 1, one * 2] {
        let budget = budget(limit);
        budget.cache_streams(bytes, &streams);
        if limit < one {
            assert!(budget.stream_index(bytes).is_none());
        } else {
            let index = budget.stream_index(bytes).unwrap();
            assert_eq!(index.for_object(bytes, 1).unwrap().1, b"abc");
            assert_eq!(
                index
                    .at_offset(bytes, streams[0].object_offset.unwrap())
                    .unwrap()
                    .1,
                b"abc"
            );
            drop(index);
            assert!(budget.check().is_ok());
            budget.cache_streams(bytes, &streams);
            assert_eq!(budget.stream_index(bytes).is_some(), limit == one * 2);
        }
        if limit == one * 2 {
            assert!(budget.check().is_ok());
        } else {
            assert!(matches!(
                budget.check(),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    ..
                })
            ));
            assert!(budget.is_stopped());
        }
    }
}

/// 同番号の古い世代と新しい世代を持ち、末尾xrefが新しい方を選ぶ入力。
fn revised_streams() -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n1 0 obj << /Length 3 >> stream\nold\nendstream endobj\n".to_vec();
    let current = bytes.len();
    bytes.extend_from_slice(b"1 1 obj << /Length 3 >> stream\nnew\nendstream endobj\n");
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 2\n0000000000 65535 f \n{current:010} 00001 n \ntrailer << /Size 2 >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    bytes
}

/// 除外objectの範囲も確保前に予約し、番号/offset配列の容量を再利用する。
#[test]
fn inactive_ranges_share_metadata_capacity() {
    let bytes = revised_streams();
    let streams = spans(&bytes);
    let base = streams.len()
        * (std::mem::size_of::<StreamRange>()
            + std::mem::size_of::<(u32, usize)>()
            + std::mem::size_of::<(usize, usize)>());
    let required = base + std::mem::size_of::<Range<usize>>();
    for limit in [required - 1, required] {
        let budget = budget(limit as u64);
        let mut index = StreamIndex::new(&budget, &bytes, &streams).unwrap();
        let xref = crate::xref::XrefIndex::read(
            std::path::Path::new("revised.pdf"),
            &bytes,
            &ParseLimits::default(),
            &DecodeBudget::default(),
        )
        .unwrap();
        let result = index.activate(&budget, &bytes, xref);
        if limit == required {
            assert!(result.is_some());
            assert!(budget.check().is_ok());
            assert_eq!(index.for_object(&bytes, 1).unwrap().1, b"new");
            assert!(!index.active_stream(&streams[0]));
            assert!(index.active_stream(&streams[1]));
            assert_eq!(index.streams(&bytes).count(), 1);
            assert_eq!(index.inactive.len(), 1);
        } else {
            assert!(result.is_none());
            assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            }) if actual == required as u64));
        }
    }
}

proptest::proptest! {
    /// 重複除去前の全容量を数え、rootless入力の番号参照は最新本文を選ぶ。
    #[test]
    fn rootless_stream_lookup_preserves_latest_occurrence(
        numbers in proptest::collection::vec(1_u32..16, 1..64)
    ) {
        let mut bytes = b"%PDF-1.7\n".to_vec();
        for (position, number) in numbers.iter().enumerate() {
            bytes.extend_from_slice(format!("{number} 0 obj << /Length 1 >> stream\n").as_bytes());
            bytes.push(position as u8);
            bytes.extend_from_slice(b"\nendstream endobj\n");
        }
        let streams = spans(&bytes);
        let required = streams.len() * (std::mem::size_of::<StreamRange>()
            + std::mem::size_of::<(u32, usize)>()
            + std::mem::size_of::<(usize, usize)>());
        let exact_budget = budget(required as u64);
        exact_budget.cache_streams(&bytes, &streams);
        let index = exact_budget.stream_index(&bytes).unwrap();
        proptest::prop_assert!(exact_budget.check().is_ok());
        let unique = numbers.iter().copied().collect::<std::collections::BTreeSet<_>>();
        proptest::prop_assert_eq!(index.streams(&bytes).count(), unique.len());
        for (position, stream) in streams.iter().enumerate() {
            let number = numbers[position];
            let last = numbers.iter().rposition(|candidate| *candidate == number).unwrap();
            proptest::prop_assert_eq!(index.for_object(&bytes, number).unwrap().1, &[last as u8]);
            proptest::prop_assert_eq!(index.active_stream(stream), position == last);
            proptest::prop_assert_eq!(index.at_offset(&bytes, stream.object_offset.unwrap()).unwrap().1, &[position as u8]);
        }
        let short = budget(required as u64 - 1);
        short.cache_streams(&bytes, &streams);
        proptest::prop_assert!(short.stream_index(&bytes).is_none());
        proptest::prop_assert!(short.check().is_err());
    }
}

/// 最新の通常objectがstreamでなくなれば古いstreamを参照・列挙しない。
#[test]
fn rootless_non_stream_replacement_removes_old_stream() {
    let bytes =
        b"%PDF-1.7\n1 7 obj << /Length 3 >> stream\nold\nendstream endobj\n1 7 obj null endobj\n";
    let streams = spans(bytes);
    let target = DecodeBudget::default();
    target.cache_streams(bytes, &streams);
    let index = target.stream_index(bytes).unwrap();
    assert!(index.for_object(bytes, 1).is_none());
    assert!(!index.active_stream(&streams[0]));
    assert_eq!(index.streams(bytes).count(), 0);
    assert!(target.check().is_ok());
}
