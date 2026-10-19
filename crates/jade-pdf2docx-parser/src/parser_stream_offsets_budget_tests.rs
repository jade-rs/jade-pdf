//! 抽出側のoffset索引を、有効streamの容量と再作成の累積で制限する。
use super::*;

/// metadata容量のみを指定した解析予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// stream一覧の検査は独立した予算で行う。
fn spans(bytes: &[u8]) -> Vec<crate::structure::StreamSpan<'_>> {
    crate::structure::validate_structure(
        Path::new("offsets.pdf"),
        bytes,
        9,
        &ParseLimits::default(),
        &DecodeBudget::default(),
    )
    .unwrap()
}

/// stream索引の準備に必要な要求容量を、予算エラーから段階的に測る。
fn preparation_capacity(bytes: &[u8]) -> u64 {
    let streams = spans(bytes);
    let mut capacity = 0;
    for _ in 0..16 {
        let budget = budget(capacity);
        budget.cache_streams(bytes, &streams);
        match budget.check() {
            Ok(()) => return capacity,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) => {
                assert!(actual > capacity);
                capacity = actual;
            }
            Err(error) => panic!("stream索引の準備に失敗: {error}"),
        }
    }
    panic!("stream索引の必要容量を特定できなかった")
}

/// 入力順と重複番号を自由に指定できるxrefなしのstream入力を作る。
fn stream_pdf(numbers: &[u32]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    for number in numbers {
        bytes.extend_from_slice(
            format!("{number} 0 obj << /Length 1 >> stream\na\nendstream endobj\n").as_bytes(),
        );
    }
    bytes
}

/// 検査済みstreamでも、offset索引用の残容量0では保存しない。
#[test]
fn cached_offsets_require_additional_metadata_capacity() {
    let bytes = stream_pdf(&[1]);
    let capacity = preparation_capacity(&bytes);
    let budget = budget(capacity);
    budget.cache_streams(&bytes, &spans(&bytes));
    assert!(budget.check().is_ok());
    let offsets = stream_object_offsets(&budget, &bytes);
    assert!(offsets.get(&1).is_none());
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit, actual,
    }) if limit == capacity && actual > capacity));
}

/// 有効streamの要求容量を数え、解放後の再作成へ予算を返さない。
#[test]
fn cached_offset_capacity_is_cumulative_and_inclusive() {
    let bytes = stream_pdf(&[1, 2, 1]);
    let streams = spans(&bytes);
    let base = preparation_capacity(&bytes);
    let one = (2 * std::mem::size_of::<(u32, usize, usize)>()) as u64;
    for available in [one - 1, one, one * 2 - 1, one * 2] {
        let budget = budget(base + available);
        budget.cache_streams(&bytes, &streams);
        let offsets = stream_object_offsets(&budget, &bytes);
        if available < one {
            assert!(offsets.get(&1).is_none());
        } else {
            assert_eq!(offsets.get(&1), streams[2].object_offset.as_ref());
            assert!(budget.check().is_ok());
            drop(offsets);
            let second = stream_object_offsets(&budget, &bytes);
            assert_eq!(second.get(&1).is_some(), available == one * 2);
        }
        assert_eq!(budget.check().is_ok(), available == one * 2);
    }
}

/// 古いxref table経路にも、保存前のmetadata容量制限を適用する。
#[test]
fn legacy_table_offsets_require_metadata_capacity() {
    let bytes = b"xref\n1 1\n0000000017 00000 n\ntrailer\n<< >>";
    let budget = budget(0);
    let offsets = stream_object_offsets(&budget, bytes);
    assert!(offsets.get(&1).is_none());
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: 0, actual,
    }) if actual > 0));
}

/// 未圧縮xref stream経路も、offsetの保存前に容量不足を返す。
#[test]
fn legacy_xref_stream_offsets_require_metadata_capacity() {
    let bytes = b"1 0 obj << /Type /XRef /Size 2 /Index [1 1] /W [1 1 1] /Length 3 >> stream\n\x01\x17\x00\nendstream endobj";
    let budget = budget(0);
    let offsets = stream_object_offsets(&budget, bytes);
    assert!(offsets.get(&1).is_none());
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: 0, actual,
    }) if actual > 0));
}

/// 最新のtype 2は古いxref stream offsetを無効にし、tableのfallbackだけを残す。
#[test]
fn compressed_xref_replacement_does_not_keep_old_stream_offset() {
    let stream = b"1 0 obj << /Type /XRef /Index [1 1 1 1] /W [1 1 1] /Length 6 >> stream\n\x01\x17\x00\x02\x05\x00\nendstream endobj";
    let offsets = stream_object_offsets(&DecodeBudget::default(), stream);
    assert!(offsets.get(&1).is_none());
    let mut table = b"xref\n1 1\n0000000017 00000 n\ntrailer\n<< >>\n".to_vec();
    table.extend_from_slice(stream);
    assert_eq!(
        stream_object_offsets(&DecodeBudget::default(), &table).get(&1),
        Some(&17)
    );
}

/// 予算エラー消費後は、索引が不要な入力を含めてoffsetの復元を再開しない。
#[test]
fn consumed_failure_does_not_restart_offset_extraction() {
    let budget = budget(0);
    assert!(budget.metadata_vector::<u8>(1).is_none());
    assert!(budget.check().is_err());
    let bytes = b"xref\n1 1\n0000000017 00000 n\ntrailer\n<< >>";
    assert!(stream_object_offsets(&budget, bytes).get(&1).is_none());
    assert!(budget.is_stopped());
}

/// u32最大番号の単独entryを受理し、範囲がoverflowするsubsectionは除外する。
#[test]
fn legacy_table_object_number_boundary_does_not_panic() {
    let one = b"xref\n4294967295 1\n0000000017 00000 n\ntrailer\n<< >>";
    let overflow = b"xref\n4294967295 2\n0000000017 00000 n\n0000000031 00000 n\ntrailer\n<< >>";
    assert_eq!(
        stream_object_offsets(&DecodeBudget::default(), one).get(&u32::MAX),
        Some(&17)
    );
    assert!(
        stream_object_offsets(&DecodeBudget::default(), overflow)
            .get(&u32::MAX)
            .is_none()
    );
}

/// 壊れたxref streamは部分entryを公開せず、未圧縮payloadのコピーも不要。
#[test]
fn legacy_xref_payload_is_borrowed_and_validated_atomically() {
    let budget = DecodeBudget::new(&ParseLimits {
        max_decoded_stream_bytes: 0,
        max_total_decoded_bytes: 0,
        ..Default::default()
    });
    let valid = b"1 0 obj << /Type /XRef /Index [4294967295 1] /W [1 1 1] /Length 3 >> stream\n\x01\x17\x00\nendstream endobj";
    assert_eq!(
        stream_object_offsets(&budget, valid).get(&u32::MAX),
        Some(&23)
    );
    assert!(budget.check().is_ok());
    for dict in [
        b"/Index [1 2] /W [1 1 1]".as_slice(),
        b"/Index [1 1 2] /W [1 1 1]",
        b"/Index [1 1] /W [1 1 9]",
        b"/Index [4294967295 2] /W [1 1 1]",
    ] {
        let mut invalid = b"1 0 obj << /Type /XRef ".to_vec();
        invalid.extend_from_slice(dict);
        invalid.extend_from_slice(b" >> stream\n\x01\x17\x00\nendstream endobj");
        let offsets = stream_object_offsets(&DecodeBudget::default(), &invalid);
        assert!(offsets.get(&1).is_none());
        assert!(offsets.get(&u32::MAX).is_none());
    }
}

/// xref一覧の一時配列と結果配列の初回・倍増容量を、累積の独立モデルで数える。
#[test]
fn legacy_xref_temporary_and_final_capacity_are_inclusive() {
    let mut bytes =
        b"1 0 obj << /Type /XRef /Size 5 /Index [1 5] /W [1 1 1] /Length 15 >> stream\n".to_vec();
    for number in 1..=5 {
        bytes.extend_from_slice(&[1, number, 0]);
    }
    bytes.extend_from_slice(b"\nendstream endobj");
    let required = 12
        * (std::mem::size_of::<(u32, Option<usize>, usize)>()
            + std::mem::size_of::<(u32, usize, usize)>());
    for limit in [required - 1, required, required * 2 - 1, required * 2] {
        let budget = budget(limit as u64);
        let offsets = stream_object_offsets(&budget, &bytes);
        if limit < required {
            assert!(offsets.get(&5).is_none());
        } else {
            assert_eq!(offsets.get(&5), Some(&5));
            assert!(budget.check().is_ok());
            drop(offsets);
            assert_eq!(
                stream_object_offsets(&budget, &bytes).get(&5).is_some(),
                limit == required * 2
            );
        }
        assert_eq!(budget.check().is_ok(), limit == required * 2);
    }
}

proptest::proptest! {
    /// Rootなし入力の番号後出し優先と有効stream容量を独立mapと比較する。
    #[test]
    fn cached_offsets_match_last_occurrence_model(numbers in proptest::collection::vec(1_u32..16, 1..48)) {
        let bytes = stream_pdf(&numbers);
        let streams = spans(&bytes);
        let base = preparation_capacity(&bytes);
        let expected: BTreeMap<_, _> = streams.iter().map(|stream| (stream.object_number.unwrap(), stream.object_offset.unwrap())).collect();
        let one = (expected.len() * std::mem::size_of::<(u32, usize, usize)>()) as u64;
        let exact = budget(base + one);
        exact.cache_streams(&bytes, &streams);
        let offsets = stream_object_offsets(&exact, &bytes);
        proptest::prop_assert!(exact.check().is_ok());
        for number in 1..16 { proptest::prop_assert_eq!(offsets.get(&number), expected.get(&number)); }
        let short = budget(base + one - 1);
        short.cache_streams(&bytes, &streams);
        proptest::prop_assert!(stream_object_offsets(&short, &bytes).get(&numbers[0]).is_none());
        proptest::prop_assert!(short.check().is_err());
    }
}
