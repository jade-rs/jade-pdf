//! Contents の借用走査・初出順・保存容量・累積境界を検証する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 指定した metadata 容量だけを許可する解析ローカル予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 作業・結果配列の累積超過が公開用の失敗状態へ保持されることを確認する。
fn assert_limit(budget: &DecodeBudget, limit: u64, expected: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: found, actual
    }) if found == limit && actual == expected));
}

/// 借用走査は有効な最大番号・世代を保持し、破損や入れ子の参照を除外する。
#[test]
fn lexical_reference_boundaries_and_nested_values() {
    let source = b"<< /Type /Page /Contents [4294967295 65535 R 4294967296 0 R 2 65536 R -1 0 R 3 -1 R 4 0 Rfoo ( 91 0 R ) [92 0 R] << /X 93 0 R >> /Name 5 +0 R 6 0 R] >>";
    assert_eq!(from_page(source).collect::<Vec<_>>(), [u32::MAX, 6]);
    assert_eq!(
        from_page(b"<< /Contents 7\0% comment\n0 R/Resources << >> >>").collect::<Vec<_>>(),
        [7]
    );
}

/// 重複が多くても作業配列の全件分を確保前に予約する。
#[test]
fn duplicate_references_charge_full_working_capacity() {
    let source = b"<< /Type /Page /Contents [7 0 R 7 0 R 7 0 R] >>";
    let requested =
        (std::mem::size_of::<&[u8]>() + 3 * std::mem::size_of::<OrderedReference>()) as u64;
    let limit = requested - 1;
    let budget = budget(limit);
    assert!(page_references(&budget, source).is_empty());
    assert_limit(&budget, limit, requested);
    assert!(page_references(&budget, source).is_empty());
    assert!(budget.check().is_ok(), "失敗を消費した後も停止を保持する");
}

/// 作業配列が収まっても、重複除去後の保存配列に足りなければ停止する。
#[test]
fn final_reference_capacity_is_reserved_separately() {
    let source = b"<< /Type /Page /Contents [7 0 R 7 0 R 4 0 R] >>";
    let requested = (std::mem::size_of::<&[u8]>()
        + 3 * std::mem::size_of::<OrderedReference>()
        + 2 * std::mem::size_of::<(usize, u32)>()) as u64;
    let limit = requested - 1;
    let limited = budget(limit);
    assert!(page_references(&limited, source).is_empty());
    assert_limit(&limited, limit, requested);
    let exact = budget(requested);
    assert_eq!(page_references(&exact, source), [(0, 7), (0, 4)]);
    assert!(exact.check().is_ok());
}

/// 同じ stream を別ページで保持し、初出順・再抽出の累積境界を守る。
#[test]
fn source_order_and_repeated_extraction_are_cumulative() {
    let source = b"<< /Type /Page /Contents [7 0 R 4 0 R 7 0 R 6 0 R] >> << /Type /Page /Contents [4 0 R 7 0 R 4 0 R] >>";
    let one = (2 * std::mem::size_of::<&[u8]>()
        + 7 * std::mem::size_of::<OrderedReference>()
        + 5 * std::mem::size_of::<(usize, u32)>()) as u64;
    let expected = [(0, 7), (0, 4), (0, 6), (1, 4), (1, 7)];
    for limit in [one * 2 - 1, one * 2] {
        let budget = budget(limit);
        assert_eq!(page_references(&budget, source), expected);
        let second = page_references(&budget, source);
        if limit == one * 2 {
            assert_eq!(second, expected);
            assert!(budget.check().is_ok());
        } else {
            assert!(second.is_empty());
            assert_limit(&budget, limit, one * 2);
        }
    }
}

/// Contents がなくても、無効値だけでも参照の保存容量を要求しない。
#[test]
fn absent_or_invalid_contents_need_no_reference_capacity() {
    for source in [
        b"<< /Type /Page >>".as_slice(),
        b"<< /Type /Page /Contents [( 9 0 R ) null << /X 8 0 R >>] >>",
    ] {
        let budget = budget(std::mem::size_of::<&[u8]>() as u64);
        assert!(page_references(&budget, source).is_empty());
        assert!(budget.check().is_ok());
    }
    let zero = budget(0);
    assert!(page_references(&zero, b"<< /Type /Font /Contents 8 0 R >>").is_empty());
    assert!(zero.check().is_ok());
}

/// 参照だけの予算では、展開本文を保存する結果配列も開始しない。
#[test]
fn content_stream_output_requires_additional_metadata_capacity() {
    let source = b"<< /Type /Page /Contents 4 0 R >>\n4 0 obj\n<< /Length 3 >>\nstream\nabc\nendstream\nendobj";
    let metadata_len =
        b"<< /Type /Page /Contents 4 0 R >>\n4 0 obj\n<< /Length 3 >>\nstream\nendstream\nendobj"
            .len();
    let limit = (metadata_len
        + std::mem::size_of::<&[u8]>()
        + std::mem::size_of::<OrderedReference>()
        + std::mem::size_of::<(usize, u32)>()) as u64;
    let budget = budget(limit);
    assert!(crate::parser::extract_page_content_streams_with_page(&budget, source).is_empty());
    assert_limit(
        &budget,
        limit,
        limit + std::mem::size_of::<(usize, Vec<u8>)>() as u64,
    );
}

/// 本文一覧の境界では payload・ページを保持し、解放後の再抽出も累積する。
#[test]
fn content_stream_output_is_cumulative_and_inclusive() {
    let source = b"<< /Type /Page /Contents 4 0 R >>\n4 0 obj\n<< /Length 3 >>\nstream\nabc\nendstream\nendobj";
    let metadata_len =
        b"<< /Type /Page /Contents 4 0 R >>\n4 0 obj\n<< /Length 3 >>\nstream\nendstream\nendobj"
            .len();
    let one = (metadata_len
        + std::mem::size_of::<&[u8]>()
        + std::mem::size_of::<OrderedReference>()
        + std::mem::size_of::<(usize, u32)>()
        + std::mem::size_of::<(usize, Vec<u8>)>()) as u64;
    for limit in [one * 2 - 1, one * 2] {
        let budget = budget(limit);
        assert_eq!(
            crate::parser::extract_page_content_streams_with_page(&budget, source),
            [(0, b"abc".to_vec())]
        );
        let second = crate::parser::extract_page_content_streams_with_page(&budget, source);
        if limit == one * 2 {
            assert_eq!(second, [(0, b"abc".to_vec())]);
            assert!(budget.check().is_ok());
        } else {
            assert!(second.is_empty());
            assert_limit(&budget, limit, one * 2);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 独立した初出順モデルと、コメントを含むページ別参照・正確な容量を比較する。
    #[test]
    fn selected_references_match_first_occurrence_model(
        pages in prop::collection::vec(prop::collection::vec(1u16..30, 0..24), 0..12)
    ) {
        let mut source = String::new();
        let mut expected = Vec::new();
        let mut count = 0;
        for (page, references) in pages.iter().enumerate() {
            source.push_str("<< /Type /Page /Contents [ % 999 0 R\n");
            let mut seen = std::collections::BTreeSet::new();
            for number in references {
                source.push_str(&format!("{number} % number\n 0 R "));
                count += 1;
                if seen.insert(*number) {
                    expected.push((page, u32::from(*number)));
                }
            }
            source.push_str("] >>\n");
        }
        let capacity = (pages.len() * std::mem::size_of::<&[u8]>()
            + count * std::mem::size_of::<OrderedReference>()
            + expected.len() * std::mem::size_of::<(usize, u32)>()) as u64;
        let exact = budget(capacity);
        prop_assert_eq!(page_references(&exact, source.as_bytes()), expected);
        prop_assert!(exact.check().is_ok());
        if capacity > 0 {
            let below = budget(capacity - 1);
            prop_assert!(page_references(&below, source.as_bytes()).is_empty());
            prop_assert!(matches!(below.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", .. })), "不足予算で参照を保存しない");
        }
    }
}
