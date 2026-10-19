//! 公開ページ配列と互換ページ継承の累積容量を固定する。
use super::*;
use proptest::prelude::*;

/// 指定metadata容量だけを許可する共有予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 訪問集合の容量だけを観測するため、fixture索引を予算外で作る。
fn geometry_objects(source: &str) -> crate::page_attributes::Objects<'_> {
    crate::metadata::objects(source.as_bytes(), None)
        .map(|object| (object.number, (object.generation, object.body)))
        .collect()
}

/// fixture索引を共有し、矩形継承の訪問容量だけを検査する。
fn parse_inherited_page_box(
    budget: &DecodeBudget,
    source: &str,
    page: &str,
    name: &str,
) -> Option<ParsedBounds> {
    super::parse_inherited_page_box(budget, &geometry_objects(source), page, name)
}

/// fixture索引を共有し、回転継承の訪問容量だけを検査する。
fn parse_inherited_page_rotation(budget: &DecodeBudget, source: &str, page: &str) -> Option<i32> {
    super::parse_inherited_page_rotation(budget, &geometry_objects(source), page)
}

/// ページ順と幾何属性が異なる表示矩形を作る。
fn viewports(count: usize) -> Vec<ParsedPageViewport> {
    (0..count)
        .map(|page_number| ParsedPageViewport {
            page: ParsedPage {
                page_number,
                width: 10.0 + page_number as f32,
                height: 20.0,
                rotation: (page_number % 4) as i32 * 90,
            },
            origin_x: 30.0,
            origin_y: 40.0,
            clip_to_page: true,
        })
        .collect()
}

/// 初回全slotを予約し、1 byte不足ではページを返さない。
#[test]
fn published_pages_require_full_capacity() {
    let source = viewports(3);
    let total = (3 * size_of::<ParsedPage>()) as u64;
    for limit in [total - 1, total] {
        let target = budget(limit);
        let pages = publish_pages(&target, &source);
        if limit == total {
            assert_eq!(
                pages,
                source.iter().map(|v| v.page.clone()).collect::<Vec<_>>()
            );
            assert!(target.check().is_ok());
        } else {
            assert!(pages.is_empty());
            assert!(
                matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total)
            );
        }
    }
}

/// 再作成も累積予約し、エラー取得後も公開処理を再開しない。
#[test]
fn published_pages_recreation_is_cumulative_and_stopped() {
    let source = viewports(2);
    let total = (2 * size_of::<ParsedPage>()) as u64;
    let target = budget(total * 2 - 1);
    assert_eq!(publish_pages(&target, &source).len(), 2);
    assert!(publish_pages(&target, &source).is_empty());
    assert!(
        matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total * 2)
    );
    assert!(publish_pages(&target, &source).is_empty());
}

/// 空ページ配列に保存容量は不要である。
#[test]
fn empty_page_publication_needs_no_capacity() {
    let target = budget(0);
    assert!(publish_pages(&target, &[]).is_empty());
    assert!(target.check().is_ok());
}

/// 継承する親番号の初回4slotを確保前に予約する。
#[test]
fn inherited_geometry_requires_visited_capacity() {
    let source = "1 0 obj << /MediaBox [0 0 10 20] /Rotate 90 >> endobj";
    let page = "<< /Parent 1 0 R >>";
    let total = (4 * size_of::<Option<u32>>()) as u64;
    for rotation in [false, true] {
        for limit in [total - 1, total] {
            let target = budget(limit);
            let present = if rotation {
                parse_inherited_page_rotation(&target, source, page).is_some()
            } else {
                parse_inherited_page_box(&target, source, page, "/MediaBox").is_some()
            };
            assert_eq!(present, limit == total);
            if limit < total {
                assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes", actual, ..
                }) if actual == total));
                let direct = "<< /MediaBox [0 0 10 20] /Rotate 90 >>";
                assert!(parse_inherited_page_box(&target, source, direct, "/MediaBox").is_none());
                assert!(parse_inherited_page_rotation(&target, source, direct).is_none());
            } else {
                assert!(target.check().is_ok());
            }
        }
    }
}

/// 互換矩形でもコメント・不正余剰値・PDF文法外の数値を寸法に採用しない。
#[test]
fn legacy_page_box_uses_pdf_numeric_grammar() {
    let valid = "<< /MediaBox [0 0 % comment\n10 20] >>";
    assert_eq!(
        parse_page_box_from_object(valid, "/MediaBox").unwrap().x1,
        10.0
    );
    for values in [
        "0 0 1e2 20",
        "0 0 10 20 999",
        "0 0 10 20 /Other",
        "0 0 10 20 NaN",
    ] {
        assert!(
            parse_page_box_from_object(&format!("<< /MediaBox [{values}] >>"), "/MediaBox")
                .is_none(),
            "{values}"
        );
    }
}

/// 後続ページの継承で不足した場合、先行ページも部分公開しない。
#[test]
fn viewport_inheritance_failure_discards_earlier_pages() {
    let source = b"1 0 obj << /Type /Page /MediaBox [0 0 10 20] >> endobj\n2 0 obj << /Type /Page /Parent 3 0 R >> endobj\n3 0 obj << /Type /Pages /MediaBox [0 0 30 40] >> endobj";
    let probe = budget(0);
    assert!(crate::object_index::build(&probe, source).is_none());
    let Err(ParseError::ResourceLimit {
        actual: index_bytes,
        ..
    }) = probe.check()
    else {
        panic!("索引容量が未検査");
    };
    let base = (source.len() + 2 * size_of::<&[u8]>() + 2 * size_of::<ParsedPageViewport>()) as u64
        + index_bytes;
    let target = budget(base + 4 * size_of::<Option<u32>>() as u64 - 1);
    assert!(parse_page_viewports(&target, source).is_empty());
    assert!(
        matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == base + 4 * size_of::<Option<u32>>() as u64)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 公開ページ容量は件数×slotであり、継続する座標正規化の入力を変えない。
    #[test]
    fn published_pages_match_capacity_and_preserve_viewports(count in 1usize..40, repeat in 1usize..4) {
        let source = viewports(count);
        let snapshot = source.clone();
        let one = (count * size_of::<ParsedPage>()) as u64;
        for limit in [one * repeat as u64 - 1, one * repeat as u64] {
            let target = budget(limit);
            for attempt in 1..=repeat {
                let pages = publish_pages(&target, &source);
                let succeeds = attempt < repeat || limit == one * repeat as u64;
                prop_assert_eq!(pages.len(), if succeeds { count } else { 0 });
                if succeeds { for (page, viewport) in pages.iter().zip(&source) { prop_assert_eq!(page, &viewport.page); } }
            }
            prop_assert_eq!(target.check().is_ok(), limit == one * repeat as u64);
        }
        prop_assert_eq!(source, snapshot);
    }

    /// 親chainの訪問集合は半分以下の占有率の全slot・再作成を予約する。
    #[test]
    fn legacy_inheritance_capacity_model(count in 1u32..25) {
        let mut source = String::new();
        for number in 1..=count {
            source.push_str(&format!("{number} 0 obj << "));
            if number == count { source.push_str("/MediaBox [0 0 10 20]"); }
            else { source.push_str(&format!("/Parent {} 0 R", number + 1)); }
            source.push_str(" >> endobj\n");
        }
        let mut capacity = 4usize;
        let mut slots = capacity;
        while capacity / 2 < count as usize { capacity *= 2; slots += capacity; }
        let total = (slots * size_of::<Option<u32>>()) as u64;
        for limit in [total - 1, total] {
            let target = budget(limit);
            let bounds = parse_inherited_page_box(&target, &source, "<< /Parent 1 0 R >>", "/MediaBox");
            prop_assert_eq!(bounds.is_some(), limit == total);
            prop_assert_eq!(target.check().is_ok(), limit == total);
            if limit == total {
                prop_assert!(parse_inherited_page_box(&target, &source, "<< /Parent 1 0 R >>", "/MediaBox").is_none());
                prop_assert!(target.check().is_err());
            }
        }
    }
}
