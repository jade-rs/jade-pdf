//! 未対応描画の状態・集合・診断出力がmetadata予算を迂回する回帰。
use super::*;
use crate::{ParseError, ParseLimits};

/// 空Formを持つRootなしの描画入力を作る。
fn fixture(content: &str) -> Vec<u8> {
    format!("%PDF-1.7\n1 0 obj << /Type /Page /Contents 2 0 R /Resources << /XObject << /Fm 3 0 R >> >> >> endobj\n2 0 obj << /Length {} >>\nstream\n{content}\nendstream\nendobj\n3 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length 0 >>\nstream\n\nendstream\nendobj\n%%EOF", content.len()).into_bytes()
}

/// metadataだけを制限する解析予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 既存抽出と借用名索引の準備容量を、診断の状態/集合/出力から独立に求める。
fn preparation(bytes: &[u8]) -> u64 {
    let mut capacity = 0;
    for _ in 0..128 {
        let target = budget(capacity);
        let source = parser::parser_source_with_compressed_objects(&target, bytes);
        let refs = parser::page_xobject_refs_from_source(&target, &source);
        let _ = target.metadata_vector::<usize>(refs.len());
        let _ = crate::object_index::build(&target, &source);
        let _ = parser::extract_page_content_streams_with_page(&target, bytes);
        match target.check() {
            Ok(()) => return capacity,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) if actual > capacity => capacity = actual,
            other => panic!("unexpected preparation {other:?}"),
        }
    }
    panic!("preparation did not converge");
}

/// 完全な予算では診断し、不足時には予算外の診断を公開しない。
fn assert_rejected(content: &str, extra: usize) {
    let bytes = fixture(content);
    let complete = budget(u64::MAX);
    let mut report = QualityReport::default();
    collect(&complete, &bytes, &mut report);
    complete.check().unwrap();
    assert_eq!(report.issues.len(), 1);
    let target = budget(preparation(&bytes) + extra as u64);
    let mut report = QualityReport::default();
    collect(&target, &bytes, &mut report);
    assert!(report.issues.is_empty(), "容量予約前に診断を公開しない");
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 状態slotを未計上のmapへ保存しない。
#[test]
fn page_state_requires_capacity() {
    assert_rejected("BI", 0);
}

/// 状態容量があっても重複除去集合には別の予約が必要。
#[test]
fn xobject_set_requires_capacity() {
    assert_rejected("/Fm Do", 4 * std::mem::size_of::<(usize, ContentState)>());
}

/// 集合までの容量があっても診断出力slotを予算外に追加しない。
#[test]
fn xobject_output_requires_capacity() {
    assert_rejected(
        "/Fm Do",
        4 * (std::mem::size_of::<(usize, ContentState)>()
            + std::mem::size_of::<Option<(usize, u32)>>()),
    );
}

/// inline imageでも診断出力slotの容量を予約する。
#[test]
fn inline_output_requires_capacity() {
    assert_rejected("BI", 4 * std::mem::size_of::<(usize, ContentState)>());
}
