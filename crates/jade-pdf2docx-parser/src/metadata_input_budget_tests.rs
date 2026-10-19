//! metadata入力コピー・置換・圧縮object連結の確保前累積予算を固定する。
use crate::{ParseError, ParseLimits, decode::DecodeBudget};
use proptest::prelude::*;
use std::{borrow::Cow, rc::Rc};

/// metadata容量だけを変更した共有予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 最初の超過を消費し、実際に要求した全容量を確認する。
fn assert_limit(budget: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: found, actual: requested,
    }) if found == limit && requested == actual));
    assert!(budget.is_stopped());
}

/// 旧経路のコピーも確保前に残容量0で拒否する。
#[test]
fn legacy_metadata_copy_rejects_zero_capacity() {
    let source = b"<< /Type /Page >>";
    let budget = budget(0);
    assert!(crate::text::pdf_bytes_without_stream_payloads(&budget, source).is_empty());
    assert_limit(&budget, 0, source.len() as u64);
}

/// 除去後に挿入する改行を含め、実際の全コピー容量と再作成を数える。
#[test]
fn legacy_metadata_copy_capacity_is_exact_and_cumulative() {
    let source = b"prefixstreamendstreamtail";
    let expected = b"prefixstream\nendstreamtail";
    let one = expected.len() as u64;
    for limit in [one - 1, one, one * 2 - 1, one * 2] {
        let budget = budget(limit);
        let first = crate::text::pdf_bytes_without_stream_payloads(&budget, source);
        if limit < one {
            assert!(first.is_empty());
            assert_limit(&budget, limit, one);
            continue;
        }
        assert_eq!(&*first, expected);
        drop(first);
        let second = crate::text::pdf_bytes_without_stream_payloads(&budget, source);
        if limit < one * 2 {
            assert!(second.is_empty());
            assert_limit(&budget, limit, one * 2);
        } else {
            assert_eq!(&*second, expected);
            assert!(budget.check().is_ok());
        }
    }
}

/// 索引の確保を済ませても、その後の構造コピーの容量を別に予約する。
#[test]
fn indexed_metadata_copy_requires_remaining_capacity() {
    let source = b"%PDF-1.7\n1 0 obj << /Length 3 >> stream\nabc\nendstream endobj";
    let streams = crate::structure::validate_structure(
        std::path::Path::new("metadata-input.pdf"),
        source,
        8,
        &ParseLimits::default(),
        &DecodeBudget::default(),
    )
    .unwrap();
    let mut index_cost = 0;
    loop {
        let probe = budget(index_cost);
        probe.cache_streams(source, &streams);
        match probe.check() {
            Ok(()) => break,
            Err(ParseError::ResourceLimit { actual, .. }) => index_cost = actual,
            Err(error) => panic!("索引の準備失敗: {error}"),
        }
    }
    let budget = budget(index_cost);
    budget.cache_streams(source, &streams);
    assert!(budget.structural_bytes(source).is_empty());
    assert_limit(&budget, index_cost, index_cost + (source.len() - 3) as u64);
}

/// 確保不要の借用でも、消費済みの停止状態から解析を再開しない。
#[test]
fn metadata_input_stays_empty_after_consumed_failure() {
    let budget = budget(0);
    assert!(budget.metadata_vector::<u8>(1).is_none());
    assert_limit(&budget, 0, 1);
    assert!(budget.structural_bytes(b"borrowed metadata").is_empty());
    assert!(crate::text::pdf_bytes_without_stream_payloads(&budget, b"copy").is_empty());
}

/// 保存済みbodyの連結も新配列全容量を予約し、番号のformat一時配列を不要にする。
#[test]
fn compressed_metadata_append_reserves_full_replacement() {
    let source = b"<<>>";
    let body = Rc::new(b"<< /Type /Page >>".to_vec());
    let expected = b"<<>> 4294967295 0 obj << /Type /Page >> endobj ";
    let required = (source.len() + expected.len()) as u64 + shared_source_cost();
    for limit in [required - 1, required] {
        let budget = budget(limit);
        budget.cache_streams(source, &[]);
        budget.cache_compressed_objects(
            source,
            crate::object_bytes::CompressedObjects::from_entries(
                &Default::default(),
                vec![(u32::MAX, body.clone())],
            )
            .unwrap(),
        );
        let output = crate::parser::parser_source_with_compressed_objects(&budget, source);
        if limit < required {
            assert!(output.is_empty());
            assert_limit(&budget, limit, required);
        } else {
            assert_eq!(&*output, expected);
            assert!(budget.check().is_ok());
        }
    }
}

/// 再検査では合成入力の再作成と共有管理領域を累積へ数える。
#[test]
fn compressed_metadata_append_is_cumulative() {
    let source = b"<<>>";
    let body = Rc::new(b"<< /Type /Page >>".to_vec());
    let expected = b"<<>> 1 0 obj << /Type /Page >> endobj ";
    let one = (source.len() + expected.len()) as u64 + shared_source_cost();
    for limit in [one * 2 - 1, one * 2] {
        let budget = budget(limit);
        budget.cache_streams(source, &[]);
        budget.cache_compressed_objects(
            source,
            crate::object_bytes::CompressedObjects::from_entries(
                &Default::default(),
                vec![(1, body.clone())],
            )
            .unwrap(),
        );
        let first = crate::parser::parser_source_with_compressed_objects(&budget, source);
        assert_eq!(&*first, expected);
        drop(first);
        budget.cache_streams(source, &[]);
        budget.cache_compressed_objects(
            source,
            crate::object_bytes::CompressedObjects::from_entries(
                &Default::default(),
                vec![(1, body.clone())],
            )
            .unwrap(),
        );
        let second = crate::parser::parser_source_with_compressed_objects(&budget, source);
        if limit < one * 2 {
            assert!(second.is_empty());
            assert_limit(&budget, limit, one * 2);
        } else {
            assert_eq!(&*second, expected);
            assert!(budget.check().is_ok());
        }
    }
}

/// 不正UTF-8は標準置換の全出力容量を予約してからmetadataとして読む。
#[test]
fn metadata_utf8_replacement_rejects_zero_capacity() {
    let budget = budget(0);
    assert!(crate::metadata_input::text(&budget, &[0xff; 4]).is_none());
    assert_limit(&budget, 0, 12);
}

/// Encodingを含むFont探索では、不正UTF-8本文の置換容量を予約する。
#[test]
fn font_encoding_metadata_replacement_requires_capacity() {
    let budget = budget(0);
    let source = b"/Encoding \xff\xff\xff\xff";
    assert!(crate::text::parse_font_resource_encodings(&budget, source).is_empty());
    assert_limit(&budget, 0, 22);
}

/// Encodingのない本文をFont探索のためにUTF-8置換しない。
#[test]
fn font_encoding_absence_does_not_replace_binary_content() {
    let budget = budget(0);
    assert!(crate::text::parse_font_resource_encodings(&budget, &[0xff; 4]).is_empty());
    assert!(budget.check().is_ok());
}

/// 未検査stream内の不正文字列で、後続FontのEncoding探索を打ち切らない。
#[test]
fn font_encoding_probe_does_not_parse_unindexed_stream_payload() {
    let source = b"1 0 obj << /Length 1 >> stream\n(\nendstream endobj\n2 0 obj << /Type /Font /Encoding /UniGB-UCS2-H >> endobj\n3 0 obj << /Font << /F1 2 0 R >> >> endobj";
    let escaped = String::from_utf8_lossy(source).replace("/Encoding", "/Enc#6fding");
    for source in [source.as_slice(), escaped.as_bytes()] {
        let budget = DecodeBudget::default();
        let encodings = crate::text::parse_font_resource_encodings(&budget, source);
        assert_eq!(
            encodings.get("F1").map(String::as_str),
            Some("UniGB-UCS2-H")
        );
        assert!(budget.check().is_ok());
    }
}

/// 有効UTF-8は容量0でも元入力を借用し、停止後は借用しない。
#[test]
fn valid_metadata_utf8_is_borrowed_without_capacity() {
    let source = "日本語 metadata".as_bytes();
    let budget = budget(0);
    let text = crate::metadata_input::text(&budget, source).unwrap();
    assert!(matches!(text, Cow::Borrowed(_)));
    assert_eq!(text.as_ptr(), source.as_ptr());
    assert!(budget.metadata_vector::<u8>(1).is_none());
    assert_limit(&budget, 0, 1);
    assert!(crate::metadata_input::text(&budget, source).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意の不正byte列を標準と同じ単位で置換し、解放後も全容量を累積する。
    #[test]
    fn replacement_capacity_matches_standard(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
        let expected = String::from_utf8_lossy(&bytes);
        let owned = matches!(expected, Cow::Owned(_));
        let one = if owned { expected.len() as u64 } else { 0 };
        let budget = budget(one);
        let first = crate::metadata_input::text(&budget, &bytes).unwrap();
        prop_assert_eq!(&first, &expected);
        prop_assert_eq!(matches!(first, Cow::Owned(_)), owned);
        drop(first);
        let second = crate::metadata_input::text(&budget, &bytes);
        if owned {
            prop_assert!(second.is_none());
            assert_limit(&budget, one, one * 2);
            let below = self::budget(one - 1);
            prop_assert!(crate::metadata_input::text(&below, &bytes).is_none());
            assert_limit(&below, one - 1, one);
        } else {
            prop_assert_eq!(second.unwrap(), expected);
            prop_assert!(budget.check().is_ok());
        }
    }

    /// 検査済みLengthで本文を除き、不正byteや偽終端をコピーしない。
    #[test]
    fn indexed_copy_capacity_counts_only_metadata(payload in prop::collection::vec(any::<u8>(), 0..80)) {
        let prefix = format!("%PDF-1.7\n1 0 obj << /Length {} >> stream\n", payload.len());
        let suffix = b"\nendstream endobj\n";
        let mut source = prefix.as_bytes().to_vec();
        source.extend_from_slice(&payload);
        source.extend_from_slice(suffix);
        let mut expected = prefix.as_bytes().to_vec();
        expected.extend_from_slice(suffix);
        let streams = crate::structure::validate_structure(
            std::path::Path::new("metadata-model.pdf"), &source, 8,
            &ParseLimits::default(), &DecodeBudget::default(),
        ).unwrap();
        let index = crate::stream_index::StreamIndex::new(&DecodeBudget::default(), &source, &streams).unwrap();
        let one = expected.len() as u64;
        for limit in [one - 1, one, one * 2 - 1, one * 2] {
            let budget = budget(limit);
            let first = index.without_payloads_for_validation(&budget, &source);
            if limit < one {
                prop_assert!(first.is_empty());
                assert_limit(&budget, limit, one);
                continue;
            }
            prop_assert_eq!(&first, &expected);
            drop(first);
            let second = index.without_payloads_for_validation(&budget, &source);
            if limit < one * 2 {
                prop_assert!(second.is_empty());
                assert_limit(&budget, limit, one * 2);
            } else {
                prop_assert_eq!(second, expected.clone());
                prop_assert!(budget.check().is_ok());
            }
        }
    }
}

/// 検査済み元入力の完成合成本文は配列を複製せず繰り返し共有する。
#[test]
fn validated_parser_source_reuses_complete_storage() {
    let source = b"hello";
    let target = budget(source.len() as u64 + shared_source_cost());
    target.cache_streams(source, &[]);
    let first = crate::parser::parser_source_with_compressed_objects(&target, source);
    let second = crate::parser::parser_source_with_compressed_objects(&target, source);
    assert_eq!(first.as_ptr(), second.as_ptr());
    assert!(target.check().is_ok());
    let other = source.to_vec();
    assert!(crate::parser::parser_source_with_compressed_objects(&target, &other).is_empty());
    assert!(target.check().is_err());
    assert!(crate::parser::parser_source_with_compressed_objects(&target, source).is_empty());
}

/// 再検査では完成本文cacheを捨てるが、呼び出し元の共有本文は保持する。
#[test]
fn parser_source_revalidation_is_cumulative() {
    let source = b"hello";
    let cost = source.len() as u64 + shared_source_cost();
    let target = budget(2 * cost - 1);
    target.cache_streams(source, &[]);
    let first = crate::parser::parser_source_with_compressed_objects(&target, source);
    assert!(target.check().is_ok());
    target.cache_streams(source, &[]);
    assert!(crate::parser::parser_source_with_compressed_objects(&target, source).is_empty());
    assert_limit(&target, 2 * cost - 1, 2 * cost);
    assert_eq!(&first[..], source);
}

/// Rcの固定管理領域もtargetのpointer幅に合わせて数える。
fn shared_source_cost() -> u64 {
    (std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>()) as u64
}

/// 圧縮object本文の更新では完成cacheを破棄し、旧共有本文の所有権は保持する。
#[test]
fn parser_source_compressed_update_invalidates_cache() {
    let source = b"<<>>";
    let target = DecodeBudget::default();
    target.cache_streams(source, &[]);
    let mut first = None;
    for count in [1, 2] {
        let body = Rc::new(format!("<< /Count {count} >>").into_bytes());
        target.cache_compressed_objects(
            source,
            crate::object_bytes::CompressedObjects::from_entries(&target, vec![(1, body)]).unwrap(),
        );
        let current = crate::parser::parser_source_with_compressed_objects(&target, source);
        let expected = format!("<<>> 1 0 obj << /Count {count} >> endobj ");
        assert_eq!(&*current, expected.as_bytes());
        if count == 1 {
            first = Some(current);
        }
    }
    assert_eq!(
        &**first.as_ref().unwrap(),
        b"<<>> 1 0 obj << /Count 1 >> endobj "
    );
    assert!(target.check().is_ok());
}
