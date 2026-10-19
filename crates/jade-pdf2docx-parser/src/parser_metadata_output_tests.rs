//! 借用ページ一覧を処理した後の抽出用配列にも共有予算を適用する。
use super::*;

/// 必要な入力コピーと借用ページ一件分だけを確保できる予算を作る。
fn page_list_only_budget(input_capacity: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: (input_capacity + std::mem::size_of::<&[u8]>()) as u64,
        ..Default::default()
    })
}

/// 表示矩形の結果配列を確保する前に停止する。
#[test]
fn viewport_output_requires_additional_metadata_capacity() {
    let source = b"<< /Type /Page /MediaBox [0 0 10 10] >>";
    let budget = page_list_only_budget(source.len());
    assert!(parse_page_viewports(&budget, source).is_empty());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// Font が空でもページ順の resource 結果配列を予算外へ保存しない。
#[test]
fn font_page_output_requires_additional_metadata_capacity() {
    let source = b"<< /Type /Page /Resources << >> >>";
    let budget = page_list_only_budget(source.len());
    assert!(parse_page_font_resources(&budget, source).is_empty());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 表示矩形の再抽出と解放を累積し、inclusive 境界で表示寸法を保持する。
#[test]
fn viewport_output_is_cumulative_and_inclusive() {
    let source = b"<< /Type /Page /MediaBox [0 0 10 20] >>";
    let one = (source.len()
        + std::mem::size_of::<&[u8]>()
        + std::mem::size_of::<ParsedPageViewport>()) as u64;
    for limit in [one * 2 - 1, one * 2] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: limit,
            ..Default::default()
        });
        let first = parse_page_viewports(&budget, source);
        assert_eq!((first[0].page.width, first[0].page.height), (10., 20.));
        drop(first);
        let second = parse_page_viewports(&budget, source);
        if limit == one * 2 {
            assert_eq!(second.len(), 1);
            assert!(budget.check().is_ok());
        } else {
            assert!(second.is_empty());
            assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            }) if actual == one * 2));
        }
    }
}

/// 空 Font map のページ配列も、別呼び出しと確保全容量を累積する。
#[test]
fn font_page_output_is_cumulative_and_inclusive() {
    let source = b"<< /Type /Page /Resources << >> >>";
    let one = (source.len()
        + std::mem::size_of::<&[u8]>()
        + std::mem::size_of::<PageFontResources>()) as u64;
    for limit in [one * 2 - 1, one * 2] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: limit,
            ..Default::default()
        });
        assert_eq!(parse_page_font_resources(&budget, source).len(), 1);
        let second = parse_page_font_resources(&budget, source);
        if limit == one * 2 {
            assert_eq!(second.len(), 1);
            assert!(budget.check().is_ok());
        } else {
            assert!(second.is_empty());
            assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            }) if actual == one * 2));
        }
    }
}

/// ページ一覧だけの予算では Contents 参照の保存を開始しない。
#[test]
fn contents_reference_output_requires_additional_metadata_capacity() {
    let budget = page_list_only_budget(0);
    let source = b"<< /Type /Page /Contents [4 0 R 5 0 R 4 0 R] >>";
    assert!(parse_page_content_object_refs_with_page(&budget, source).is_empty());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// コメントと文字列に書かれた参照を実際の Contents と混同しない。
#[test]
fn contents_reference_tokens_exclude_comments_and_strings() {
    let source = b"<< /Type /Page /Contents [% 91 0 R\n ( 92 0 R ) <393320302052> 4 % object\n 0 % generation\n R 5 0 R] >>";
    assert_eq!(
        crate::content_refs::from_page(source).collect::<Vec<_>>(),
        vec![4, 5]
    );
}

/// XObject の結果と名前を metadata 予算の外で保存しない。
#[test]
fn xobject_reference_output_requires_metadata_capacity() {
    let source = b"7 0 obj << /Type /XObject /Subtype /Form >> endobj";
    let indexing = DecodeBudget::default();
    let objects = crate::object_index::build(&indexing, source).unwrap();
    for limit in [0, std::mem::size_of::<(String, u32)>() as u64] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: limit,
            ..Default::default()
        });
        assert!(
            crate::xobject_refs::from_resources(
                &budget,
                &objects,
                b"<< /XObject << /Im 7 0 R >> >>"
            )
            .is_empty()
        );
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                ..
            })
        ));
    }
}

/// Root のない入力でもコメント・文字列・別の辞書の参照を拾わない。
#[test]
fn xobject_fallback_is_scoped_to_resource_dictionary() {
    let source = b"1 0 obj << /Type /Page /Resources << /XObject <<
        % /Fake 8 0 R
        /Real 7 % object
0 % generation
R
        /Text (/False 8 0 R)
        >> /Other << /False 8 0 R >> >> >> endobj
        7 0 obj << /Type /XObject /Subtype /Form >> endobj
        8 0 obj << /Type /XObject /Subtype /Form >> endobj";
    let budget = DecodeBudget::default();
    assert_eq!(
        page_xobject_refs_from_source(&budget, source),
        [(0, "Real".into(), 7)]
    );
    assert!(budget.check().is_ok());
}

/// object 索引とページ借用だけの予算ではページ別 XObject を保存しない。
#[test]
fn xobject_page_output_requires_additional_metadata_capacity() {
    let source = b"1 0 obj << /Type /Page /Resources << /XObject << /Im 7 0 R >> >> >> endobj
        7 0 obj << /Type /XObject /Subtype /Form >> endobj";
    let indexing = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: 0,
        ..Default::default()
    });
    assert!(crate::object_index::build(&indexing, source).is_none());
    let Err(ParseError::ResourceLimit { actual, .. }) = indexing.check() else {
        panic!("索引の要求容量が報告されない");
    };
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: actual + std::mem::size_of::<&[u8]>() as u64,
        ..Default::default()
    });
    assert!(page_xobject_refs_from_source(&budget, source).is_empty());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}
