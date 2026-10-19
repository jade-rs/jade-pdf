//! Form訪問の本文連結とcache保存がmetadata予算を迂回する回帰。
use super::*;
use crate::{ParseError, ParseLimits};

/// 空Formを一回呼ぶRootなしの最小互換入力を作る。
pub(super) fn fixture() -> Vec<u8> {
    b"%PDF-1.7\n1 0 obj << /Type /Page /Contents 2 0 R /Resources << /XObject << /F 3 0 R >> >> >> endobj\n2 0 obj << /Length 5 >>\nstream\n/F Do\nendstream\nendobj\n3 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length 0 >>\nstream\n\nendstream\nendobj\n%%EOF".to_vec()
}

/// 検証済みPageを入口から受け取った状態にして、訪問中の保存だけを検査する。
pub(super) fn budget(capacity: u64) -> DecodeBudget {
    let target = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    });
    target.cache_pages(Some(vec![crate::page_attributes::Page {
        number: 1,
        media_box: [0., 0., 100., 100.],
        crop_box: None,
        rotation: 0,
        resources: Some(crate::page_attributes::ResourceLocation::Inline(1)),
    }]));
    target
}

/// 訪問準備の索引・Resources・本文取得だけに必要なmetadata容量を求める。
pub(super) fn preparation(bytes: &[u8]) -> u64 {
    let mut capacity = 0;
    for _ in 0..128 {
        let budget = budget(capacity);
        let source = parser::parser_source_with_compressed_objects(&budget, bytes);
        let _ = crate::object_index::build(&budget, &source);
        let _ = budget.page_resources(&source);
        let _ = parser::stream_object_offsets(&budget, bytes);
        let _ = parser::extract_page_content_streams_with_page(&budget, bytes);
        match budget.check() {
            Ok(()) => return capacity,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) if actual > capacity => capacity = actual,
            other => panic!("unexpected preparation: {other:?}"),
        }
    }
    panic!("preparation did not converge");
}

/// 本文保存段階の容量が足りなければForm描画を通知しない。
fn assert_rejected(extra: u64) {
    let bytes = fixture();
    let mut complete = 0;
    let unrestricted = budget(ParseLimits::default().max_total_metadata_workspace_bytes);
    visit(
        &unrestricted,
        &bytes,
        &mut QualityReport::default(),
        |_, _, _, _, _, _| complete += 1,
    );
    unrestricted.check().unwrap();
    assert_eq!(
        complete, 1,
        "fixture must visit its Form before imposing the budget"
    );
    let budget = budget(preparation(&bytes) + extra);
    let mut visits = 0;
    visit(
        &budget,
        &bytes,
        &mut QualityReport::default(),
        |_, _, _, _, _, _| visits += 1,
    );
    assert_eq!(visits, 0);
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 準備索引だけの容量では連結するページ本文を保存できない。
#[test]
fn page_join_requires_metadata_capacity() {
    assert_rejected(0);
}

/// streamの順番索引とページslotだけでは本文byteを保存できない。
#[test]
fn page_join_bytes_require_metadata_capacity() {
    assert_rejected(
        (std::mem::size_of::<usize>() + std::mem::size_of::<(usize, Rc<Vec<u8>>)>()) as u64,
    );
}

/// ページ本文と共有本体まで保存できてもForm cacheのslotは別途予約する。
#[test]
fn payload_cache_requires_metadata_capacity() {
    let page = std::mem::size_of::<usize>()
        + std::mem::size_of::<(usize, Rc<Vec<u8>>)>()
        + 6
        + std::mem::size_of::<Vec<u8>>()
        + 2 * std::mem::size_of::<usize>();
    assert_rejected(page as u64);
}
