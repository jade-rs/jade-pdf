//! 検証用 graph/参照追跡が索引と同じ累積容量予算を守る回帰。
use crate::page_attribute_fixture as pdf;
use crate::{ParseError, ParseLimits, decode::DecodeBudget, object_index::Objects};
use std::path::Path;

/// 索引 fixture の確保とは独立した検証用予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 必ず同じ typed な予算超過を呼び出し元へ返す。
fn assert_limit<T>(result: Result<T, ParseError>, limit: u64) {
    assert!(matches!(result, Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: found, actual
    }) if found == limit && actual > limit));
}

/// 空の resource 辞書でも作業 queue の初回確保前に検査する。
#[test]
fn resource_queue_respects_metadata_budget() {
    assert_limit(
        crate::page_resources::validate(
            Path::new("resource.pdf"),
            &Objects::new(),
            b"<< >>",
            &ParseLimits::default(),
            &budget(0),
        ),
        0,
    );
}

/// 間接属性の訪問済み参照を予算外の set へ保存しない。
#[test]
fn attribute_reference_set_respects_metadata_budget() {
    let objects = Objects::from([(1, (0, b"90".as_slice()))]);
    assert_limit(
        crate::page_attributes::resolve_value(
            Path::new("attribute.pdf"),
            &objects,
            b"1 0 R",
            &ParseLimits::default(),
            &budget(0),
        ),
        0,
    );
}

/// Form の直接 BBox 配列にも確保前の累積検査を適用する。
#[test]
fn form_geometry_array_respects_metadata_budget() {
    assert_limit(
        crate::form_attributes::validate(
            Path::new("form.pdf"),
            &Objects::new(),
            b"<< /BBox [0 0 10 10] >>",
            &ParseLimits::default(),
            &budget(0),
        ),
        0,
    );
}

/// 構造検査とstream索引を保存する最小容量を、公開される超過量から求める。
fn preparation_capacity(bytes: &[u8]) -> u64 {
    let mut capacity = 0;
    for _ in 0..64 {
        let candidate = budget(capacity);
        let result = crate::structure::validate_structure(
            Path::new("tree.pdf"),
            bytes,
            9,
            &ParseLimits::default(),
            &candidate,
        )
        .and_then(|streams| {
            candidate.cache_streams(bytes, &streams);
            candidate.check()
        });
        match result {
            Ok(()) => return capacity,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) => {
                assert!(actual > capacity);
                capacity = actual;
            }
            result => panic!("構造検査・stream索引の準備に失敗: {result:?}"),
        }
    }
    panic!("構造検査・stream索引の累積境界へ到達しない");
}

/// 構造検査・stream索引・object索引だけの予算ではpage treeの最初の作業配列を確保しない。
#[test]
fn page_tree_workspace_respects_metadata_budget() {
    let bytes = pdf::pdf(&pdf::pages("/MediaBox [0 0 500 500]"));
    let first = budget(0);
    assert!(crate::object_index::build(&first, &bytes).is_none());
    let Err(ParseError::ResourceLimit {
        actual: index_bytes,
        ..
    }) = first.check()
    else {
        panic!("索引の要求容量が不明");
    };
    let capacity = preparation_capacity(&bytes) + index_bytes;
    let limited = budget(capacity);
    let limits = ParseLimits::default();
    let streams =
        crate::structure::validate_structure(Path::new("tree.pdf"), &bytes, 9, &limits, &limited)
            .unwrap();
    limited.cache_streams(&bytes, &streams);
    assert!(limited.check().is_ok());
    let first_stack = 4 * std::mem::size_of::<(
        (u32, u16),
        Option<(u32, u16)>,
        usize,
        crate::page_attributes::Inherited,
    )>() as u64;
    assert!(matches!(
        crate::page_tree::resolve(Path::new("tree.pdf"), &bytes, &bytes, &limited, &limits, &mut crate::PdfVersion { major: 1, minor: 7 }),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", limit, actual,
        }) if limit == capacity && actual == capacity + first_stack
    ));
}
