//! 抽出用ページ・Resources 一覧の確保前予算を固定する。
use crate::{
    ParseError, ParseLimits,
    decode::DecodeBudget,
    page_attributes::{Page, ResourceLocation},
};
use proptest::prelude::*;

/// 索引と抽出配列で共有する予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 抽出が保存した最初の超過を確認する。
fn assert_limit(budget: &DecodeBudget, limit: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: found, actual
    }) if found == limit && actual > limit));
}

/// fixture の所属ページを順序どおり保存する。
fn pages(numbers: &[u32]) -> Vec<Page> {
    numbers
        .iter()
        .map(|&number| Page {
            number,
            media_box: [0., 0., 10., 10.],
            crop_box: None,
            rotation: 0,
            resources: None,
        })
        .collect()
}

/// fixture の索引要求容量だけを調べる。
fn index_capacity(source: &[u8]) -> u64 {
    let limited = budget(0);
    assert!(crate::object_index::build(&limited, source).is_none());
    let Err(ParseError::ResourceLimit { actual, .. }) = limited.check() else {
        panic!("索引容量が不明");
    };
    actual
}

/// Root のない入力の一覧も、確保前に停止し失敗を公開検査へ残す。
#[test]
fn legacy_page_list_rejects_zero_budget() {
    let limited = budget(0);
    assert!(limited.page_dictionaries(b"<< /Type /Page >>").is_empty());
    assert_limit(&limited, 0);
}

/// 正式な所属ページの再選択では索引以外の出力配列も数える。
#[test]
fn formal_page_list_requires_output_capacity() {
    let source = b"3 0 obj << /Type /Page >> endobj\n4 0 obj << /Type /Page >> endobj";
    let capacity = index_capacity(source);
    let limited = budget(capacity);
    limited.cache_pages(Some(pages(&[4, 3])));
    assert!(limited.page_dictionaries(source).is_empty());
    assert_limit(&limited, capacity);
}

/// Resources が全て欠落していてもページ順の配列容量は加算する。
#[test]
fn resource_page_list_requires_output_capacity() {
    let source = b"3 0 obj << /Type /Page >> endobj\n4 0 obj << /Type /Page >> endobj";
    let capacity = index_capacity(source);
    let limited = budget(capacity);
    limited.cache_pages(Some(pages(&[4, 3])));
    assert!(limited.page_resources(source).is_none());
    assert_limit(&limited, capacity);
}

/// 選択する Page がなければ非 Page 辞書の件数による配列を確保しない。
#[test]
fn non_page_dictionaries_need_no_list_allocation() {
    let source =
        b"<< /Type /Font /Nested << /Type /Page >> >>\n% << /Type /Page >>\n(<< /Type /Page >>)\n"
            .repeat(128);
    let limited = budget(0);
    assert!(limited.page_dictionaries(&source).is_empty());
    assert!(limited.check().is_ok());
}

/// 所属順・orphan 除外と inclusive 境界を、索引と別の配列容量で保証する。
#[test]
fn formal_selection_preserves_order_at_exact_capacity() {
    let source = b"3 0 obj << /Type /Page /Id 3 >> endobj\n4 0 obj << /Type /Page /Id 4 >> endobj\n5 0 obj << /Type /Page /Id 5 >> endobj";
    let capacity = index_capacity(source) + 2 * std::mem::size_of::<&[u8]>() as u64;
    let short = budget(capacity - 1);
    short.cache_pages(Some(pages(&[4, 3])));
    assert!(short.page_dictionaries(source).is_empty());
    assert_limit(&short, capacity - 1);
    let exact = budget(capacity);
    exact.cache_pages(Some(pages(&[4, 3])));
    let found = exact.page_dictionaries(source);
    assert_eq!(
        found
            .iter()
            .map(|dictionary| crate::metadata::value(dictionary, b"Id").unwrap())
            .collect::<Vec<_>>(),
        [b"4".as_slice(), b"3".as_slice()]
    );
    assert!(exact.check().is_ok());
}

/// 空・直接・間接 Resources のページ位置を借用のまま保持する。
#[test]
fn resource_selection_preserves_empty_and_inherited_positions() {
    let source = b"3 0 obj << /Type /Page /Resources << /Font << >> >> >> endobj\n4 0 obj << /Type /Page >> endobj\n9 0 obj << /XObject << >> >> endobj";
    let capacity = index_capacity(source) + 3 * std::mem::size_of::<Option<&[u8]>>() as u64;
    let limited = budget(capacity);
    let mut selected = pages(&[4, 3, 4]);
    selected[1].resources = Some(ResourceLocation::Inline(3));
    selected[2].resources = Some(ResourceLocation::Indirect(9));
    limited.cache_pages(Some(selected));
    let found = limited.page_resources(source).unwrap();
    assert_eq!(
        found,
        [
            None,
            Some(b"<< /Font << >> >>".as_slice()),
            Some(b"<< /XObject << >> >>".as_slice())
        ]
    );
    assert!(limited.check().is_ok());
    for value in found.into_iter().flatten() {
        let offset = value.as_ptr() as usize - source.as_ptr() as usize;
        assert!(offset + value.len() <= source.len());
    }
}

/// 同じ借用入力の再抽出も累積し、消費済みの失敗後に再開しない。
#[test]
fn repeated_legacy_selection_is_cumulative_and_stays_stopped() {
    let source = b"<< /Type /Page >> << /Type /Page >>";
    let one = 2 * std::mem::size_of::<&[u8]>() as u64;
    for limit in [one * 2 - 1, one * 2] {
        let limited = budget(limit);
        assert_eq!(limited.page_dictionaries(source).len(), 2);
        let second = limited.page_dictionaries(source);
        if limit == one * 2 {
            assert_eq!(second.len(), 2);
            assert!(limited.check().is_ok());
        } else {
            assert!(second.is_empty());
            assert_limit(&limited, limit);
            assert!(limited.page_dictionaries(source).is_empty());
            assert!(limited.page_resources(source).is_none());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// 入れ子・文字列・コメントの偽 Page を除き、独立した入力順の期待値と比較する。
    #[test]
    fn legacy_selection_matches_page_flags(flags in prop::collection::vec(any::<bool>(), 0..64)) {
        let mut source = String::new();
        let mut expected = Vec::new();
        for (index, page) in flags.iter().enumerate() {
            let kind = if *page { "Page" } else { "Font" };
            source.push_str(&format!("<< /Type /{kind} /Id {index} /Nested << /Type /Page >> >>\n% << /Type /Page >>\n(<< /Type /Page >>)\n"));
            if *page { expected.push(index); }
        }
        let capacity = expected.len() as u64 * std::mem::size_of::<&[u8]>() as u64;
        let limited = budget(capacity);
        let selected = limited.page_dictionaries(source.as_bytes());
        let actual = selected.iter().map(|dictionary| {
            std::str::from_utf8(crate::metadata::value(dictionary, b"Id").unwrap()).unwrap().parse::<usize>().unwrap()
        }).collect::<Vec<_>>();
        prop_assert_eq!(actual, expected);
        prop_assert!(limited.check().is_ok());
        for dictionary in selected {
            let offset = dictionary.as_ptr() as usize - source.as_ptr() as usize;
            prop_assert!(offset + dictionary.len() <= source.len());
        }
    }
}
