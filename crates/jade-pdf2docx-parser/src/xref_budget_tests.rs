//! xref履歴と、その検査用一時配列の共有metadata容量を検証する。
use super::*;
use crate::decode::DecodeBudget;

/// metadata容量だけを指定し、xref検査と同じ共有予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 同じ番号を更新するPrev chainと、二つの整数objectを作る。
fn table_pdf(revisions: usize) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let old = bytes.len();
    bytes.extend_from_slice(b"1 0 obj 7 endobj\n");
    let new = bytes.len();
    bytes.extend_from_slice(b"1 0 obj 9 endobj\n");
    let mut previous = None;
    for revision in 0..revisions {
        let start = bytes.len();
        let selected = if revision % 2 == 0 { old } else { new };
        let prev = previous
            .map(|offset| format!(" /Prev {offset}"))
            .unwrap_or_default();
        bytes.extend_from_slice(
            format!("xref\n1 1\n{selected:010} 00000 n\ntrailer << /Size 2{prev} >>\n").as_bytes(),
        );
        previous = Some(start);
    }
    bytes.extend_from_slice(format!("startxref\n{}\n%%EOF\n", previous.unwrap()).as_bytes());
    bytes
}

/// W/Indexと未圧縮本文を持つ最小xref streamを作る。
fn stream_pdf() -> Vec<u8> {
    let mut bytes =
        b"%PDF-1.7\n2 0 obj << /Type /XRef /Size 2 /W [1 2 1] /Index [1 1] /Length 4 >> stream\n"
            .to_vec();
    bytes.extend_from_slice(&[1, 0, 9, 0]);
    bytes.extend_from_slice(b"\nendstream endobj\nstartxref\n9\n%%EOF\n");
    bytes
}

/// 入力を検査し、正式xref索引を返す。
fn read(bytes: &[u8], budget: &DecodeBudget) -> Result<Option<XrefIndex>, ParseError> {
    XrefIndex::read(
        Path::new("xref-budget.pdf"),
        bytes,
        &ParseLimits::default(),
        budget,
    )
}

/// 実際の容量超過値を次の上限へ進め、独立した予算で最小成功容量を測る。
fn required_capacity(bytes: &[u8]) -> u64 {
    let mut limit = 0;
    for _ in 0..128 {
        match read(bytes, &budget(limit)) {
            Ok(Some(_)) => return limit,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) => {
                assert!(actual > limit);
                limit = actual;
            }
            _ => panic!("unexpected xref result"),
        }
    }
    panic!("xref metadata capacity did not converge");
}

/// 正式tableの履歴とPrev訪問集合は、容量0で保存を開始しない。
#[test]
fn table_history_requires_metadata_capacity() {
    assert!(
        matches!(read(&table_pdf(2), &budget(0)), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: 0, actual,
    }) if actual > 0)
    );
}

/// W/Indexの整数配列は、xref entry保存前でも共有予算を迂回しない。
#[test]
fn indexed_xref_arrays_require_metadata_capacity() {
    let bytes = stream_pdf();
    let result = crate::indexed_stream::read(
        Path::new("xref-budget.pdf"),
        &bytes,
        9,
        &ParseLimits::default(),
        &budget(0),
    );
    assert!(matches!(result, Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: 0, actual,
    }) if actual > 0));
}

/// 解放した履歴と再検査の一時配列も返却せず、累積のinclusive境界を守る。
#[test]
fn repeated_xref_history_capacity_is_cumulative() {
    for bytes in [table_pdf(3), stream_pdf()] {
        let one = required_capacity(&bytes);
        assert!(one > 0);
        for limit in [one * 2 - 1, one * 2] {
            let shared = budget(limit);
            drop(read(&bytes, &shared).unwrap());
            let second = read(&bytes, &shared);
            assert_eq!(second.is_ok(), limit == one * 2);
            if let Err(error) = second {
                assert!(matches!(error, ParseError::ResourceLimit {
                    resource: "metadata workspace bytes", actual, ..
                } if actual == one * 2));
            }
        }
    }
}

/// エラー消費後は、xrefのない入力でも検査を再開しない。
#[test]
fn consumed_failure_does_not_restart_xref_validation() {
    let shared = budget(0);
    assert!(shared.metadata_vector::<u8>(1).is_none());
    assert!(shared.check().is_err());
    assert!(read(b"%PDF-1.7\n", &shared).is_err());
    assert!(shared.is_stopped());
}

/// 初回4要素と倍増時の新配列全体を累積へ数える独立モデル。
fn vector_slots(length: usize) -> usize {
    let mut capacity = 0;
    let mut slots = 0;
    while capacity < length {
        capacity = (capacity * 2).max(4);
        slots += capacity;
    }
    slots
}

/// 訪問集合は占有率半分を超える前に、新配列全体を予約する。
fn set_slots(length: usize) -> usize {
    vector_slots(length * 2)
}

/// 同じ物理offsetの候補を重複除去する前に全件予約する。
#[test]
fn historical_offsets_capacity_is_inclusive_before_deduplication() {
    let bytes = table_pdf(4);
    let required = set_slots(4) * std::mem::size_of::<Option<usize>>()
        + 4 * vector_slots(1) * std::mem::size_of::<storage::StagedEntry>()
        + vector_slots(4) * std::mem::size_of::<storage::RevisionEntry>()
        + 3 * std::mem::size_of::<storage::HistoricalOffset>();
    assert_eq!(required_capacity(&bytes), required as u64);
    for limit in [required - 1, required] {
        let result = read(&bytes, &budget(limit as u64));
        assert_eq!(result.is_ok(), limit == required);
        if let Err(error) = result {
            assert!(matches!(error, ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            } if actual == required as u64));
        }
    }
}

/// tableの重複番号も一時容量へ数え、各入力元の最初のentryを保持する。
#[test]
fn duplicate_revision_entries_are_charged_before_deduplication() {
    let required = vector_slots(5) * std::mem::size_of::<storage::StagedEntry>();
    for limit in [required - 1, required] {
        let shared = budget(limit as u64);
        let mut entries = RevisionEntries::default();
        entries.insert(&shared, 1, Entry::Free, false).unwrap();
        let result = (0..4).try_for_each(|offset| {
            entries.insert(
                &shared,
                1,
                Entry::Uncompressed {
                    generation: 0,
                    offset,
                },
                false,
            )
        });
        assert_eq!(result.is_ok(), limit == required);
        if result.is_ok() {
            let mut selected = entries.into_entries();
            assert!(matches!(selected.next(), Some((1, Entry::Free))));
            assert!(selected.next().is_none());
        } else {
            assert!(matches!(result, Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            }) if actual == required as u64));
        }
    }
}

/// 未圧縮xrefのW/Index・一時entry・履歴・訪問集合を独立に計算する。
#[test]
fn xref_stream_capacity_matches_independent_allocation_model() {
    let required = set_slots(1) * std::mem::size_of::<Option<usize>>()
        + (vector_slots(3) + vector_slots(2)) * std::mem::size_of::<usize>()
        + vector_slots(1) * std::mem::size_of::<storage::StagedEntry>()
        + vector_slots(1) * std::mem::size_of::<storage::RevisionEntry>();
    assert_eq!(required_capacity(&stream_pdf()), required as u64);
}

use proptest::prelude::*;
use std::collections::BTreeMap;

/// 番号ごとに物理objectを置き、削除・更新・同一revision内の重複を作る。
fn model_pdf(revisions: &[Vec<(usize, bool)>]) -> (Vec<u8>, BTreeMap<usize, usize>) {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = BTreeMap::new();
    for number in 1..8 {
        offsets.insert(number, bytes.len());
        bytes.extend_from_slice(format!("{number} 0 obj {number} endobj\n").as_bytes());
    }
    let mut previous = None;
    for rows in revisions {
        let start = bytes.len();
        bytes.extend_from_slice(b"xref\n");
        for &(number, live) in rows {
            let (offset, state) = if live {
                (offsets[&number], 'n')
            } else {
                (0, 'f')
            };
            bytes.extend_from_slice(format!("{number} 1\n{offset:010} 00000 {state}\n").as_bytes());
        }
        let prev = previous
            .map(|offset| format!(" /Prev {offset}"))
            .unwrap_or_default();
        bytes.extend_from_slice(format!("trailer << /Size 8{prev} >>\n").as_bytes());
        previous = Some(start);
    }
    bytes.extend_from_slice(format!("startxref\n{}\n%%EOF\n", previous.unwrap()).as_bytes());
    (bytes, offsets)
}

proptest! {
    /// 削除・疎な更新・重複と旧offsetの参照時点をmapモデルで照合する。
    #[test]
    fn xref_history_matches_revision_and_capacity_model(
        revisions in prop::collection::vec(prop::collection::vec((1_usize..8, any::<bool>()), 1..13), 1..9),
    ) {
        let (bytes, offsets) = model_pdf(&revisions);
        let mut model: BTreeMap<usize, Vec<(usize, bool)>> = BTreeMap::new();
        let mut staged = 0;
        let mut count = 0;
        for (revision, rows) in revisions.iter().rev().enumerate() {
            staged += vector_slots(rows.len());
            let mut unique = BTreeMap::new();
            for &(number, live) in rows {
                unique.entry(number).or_insert(live);
            }
            count += unique.len();
            for (number, live) in unique {
                model.entry(number).or_default().push((revision, live));
            }
        }
        let old_candidates = model.values().map(|history| {
            history.windows(2).filter(|pair| pair[1].1).count()
        }).sum::<usize>();
        let required = set_slots(revisions.len()) * std::mem::size_of::<Option<usize>>()
            + staged * std::mem::size_of::<storage::StagedEntry>()
            + vector_slots(count) * std::mem::size_of::<storage::RevisionEntry>()
            + old_candidates * std::mem::size_of::<storage::HistoricalOffset>();
        prop_assert_eq!(required_capacity(&bytes), required as u64);
        let exact = budget(required as u64);
        let index = read(&bytes, &exact).unwrap().unwrap();
        prop_assert!(exact.check().is_ok());
        for number in 1..8 {
            let history = model.get(&number);
            for revision in 0..=revisions.len() {
                let expected = history.and_then(|history| history.iter().find(|(r, _)| *r >= revision));
                let actual = index.entry_at(number, revision);
                match expected {
                    Some((_, true)) => prop_assert!(matches!(actual, Some(Entry::Uncompressed { generation: 0, offset }) if *offset == offsets[&number]), "selected physical object"),
                    Some((_, false)) => prop_assert!(matches!(actual, Some(Entry::Free))),
                    None => prop_assert!(actual.is_none()),
                }
            }
            let expected = if history.is_some_and(|history| history[0].1) {
                0
            } else {
                history.and_then(|history| history.windows(2).find(|pair| pair[1].1)).map(|pair| pair[0].0 + 1).unwrap_or(0)
            };
            prop_assert_eq!(index.stream_revision(number as u32, 0, offsets[&number]), expected);
        }
        prop_assert_eq!(index.compressed().count(), 0);
        prop_assert!(matches!(read(&bytes, &budget(required as u64 - 1)), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", actual, ..
        }) if actual == required as u64), "inclusive metadata limit");
    }
}
