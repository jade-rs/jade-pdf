//! previewに渡すstream保存slotと未圧縮本文の容量を固定する。
use super::*;
use crate::{ParseError, ParseLimits};

/// stream一覧の保存容量だけを指定する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// Root/xrefのない旧経路で、Imageと非Imageを区別する入力。
const PDF: &[u8] = b"1 0 obj << /Length 1 >> stream\nx\nendstream endobj\n2 0 obj << /Subtype /Image /Length 1 >> stream\ny\nendstream endobj\n";

/// 非Imageの初回保存slotだけを予約し、Image本文には容量を消費しない。
#[test]
fn preview_stream_slots_are_inclusive() {
    let cost = (4 * std::mem::size_of::<Vec<u8>>()) as u64;
    let enough = budget(cost);
    assert_eq!(
        extract_non_image_stream_payloads(&enough, PDF),
        vec![b"x".to_vec()]
    );
    assert!(enough.check().is_ok());
    let short = budget(cost - 1);
    assert!(extract_non_image_stream_payloads(&short, PDF).is_empty());
    assert!(matches!(short.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", actual, ..
    }) if actual == cost));
    assert!(extract_non_image_stream_payloads(&short, PDF).is_empty());
}

/// slotは解放後も累積し、停止後の空入力も処理を再開しない。
#[test]
fn preview_stream_recreation_is_cumulative() {
    let cost = (4 * std::mem::size_of::<Vec<u8>>()) as u64;
    let target = budget(2 * cost - 1);
    assert_eq!(extract_non_image_stream_payloads(&target, PDF).len(), 1);
    assert!(extract_non_image_stream_payloads(&target, PDF).is_empty());
    assert!(
        matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == 2 * cost)
    );
    assert!(extract_non_image_stream_payloads(&target, PDF).is_empty());
    assert!(target.is_stopped());
}

/// 未圧縮本文にも累積展開予算を適用し、後続不足時に先行本文を破棄する。
#[test]
fn preview_plain_stream_failure_discards_prior_payloads() {
    let target = DecodeBudget::new(&ParseLimits {
        max_total_decoded_bytes: 1,
        ..Default::default()
    });
    let bytes = b"1 0 obj << /Length 1 >> stream\nx\nendstream endobj\n2 0 obj << /Length 1 >> stream\ny\nendstream endobj\n";
    assert!(extract_non_image_stream_payloads(&target, bytes).is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "total decoded bytes",
            limit: 1,
            actual: 2,
        })
    ));
    assert!(extract_non_image_stream_payloads(&target, PDF).is_empty());
}

/// 直接Lengthで区切り、本文末尾の改行と本文内のendstreamを保持する。
#[test]
fn preview_plain_stream_preserves_binary_boundaries() {
    for payload in [b"a\r\n".as_slice(), b"endstream", b"x\nendstream\ny"] {
        let mut bytes = format!("1 0 obj << /Length {} >> stream\n", payload.len()).into_bytes();
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(b"\nendstream\nendobj");
        let target = DecodeBudget::default();
        assert_eq!(
            extract_non_image_stream_payloads(&target, &bytes),
            vec![payload.to_vec()]
        );
        assert!(target.check().is_ok());
    }
}

/// 検査済み索引の必要容量を、抽出に渡す予算とは独立して測る。
fn indexed_preparation(bytes: &[u8], spans: &[crate::structure::StreamSpan<'_>]) -> u64 {
    let mut capacity = 0;
    for _ in 0..16 {
        let target = budget(capacity);
        target.cache_streams(bytes, spans);
        match target.check() {
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
    panic!("stream索引の容量が収束しない")
}

/// 検査済みstreamでも、保存slotを累積して停止後に本文を返さない。
#[test]
fn preview_indexed_stream_slots_are_cumulative_and_inclusive() {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    bytes.extend_from_slice(PDF);
    let spans = crate::structure::validate_structure(
        std::path::Path::new("preview.pdf"),
        &bytes,
        9,
        &ParseLimits::default(),
        &DecodeBudget::default(),
    )
    .unwrap();
    let base = indexed_preparation(&bytes, &spans);
    let cost = (4 * std::mem::size_of::<Vec<u8>>()) as u64;
    for available in [cost - 1, cost, 2 * cost - 1, 2 * cost] {
        let target = budget(base + available);
        target.cache_streams(&bytes, &spans);
        assert!(target.check().is_ok());
        let first = extract_non_image_stream_payloads(&target, &bytes);
        assert_eq!(first.len(), usize::from(available >= cost));
        if available >= cost {
            assert_eq!(first[0], b"x");
            assert!(target.check().is_ok());
            let second = extract_non_image_stream_payloads(&target, &bytes);
            assert_eq!(second.len(), usize::from(available == 2 * cost));
        }
        assert_eq!(target.check().is_ok(), available == 2 * cost);
        if available != 2 * cost {
            assert!(target.is_stopped());
            assert!(extract_non_image_stream_payloads(&target, &bytes).is_empty());
        }
    }
}

/// 保存slot拡張では新しい全容量を予約し、拡張失敗時に先行本文を破棄する。
#[test]
fn preview_stream_growth_reserves_full_capacity() {
    let bytes = (1..=5)
        .map(|number| format!("{number} 0 obj << /Length 1 >> stream\nx\nendstream endobj\n"))
        .collect::<String>();
    let cost = (12 * std::mem::size_of::<Vec<u8>>()) as u64;
    let enough = budget(cost);
    assert_eq!(
        extract_non_image_stream_payloads(&enough, bytes.as_bytes()).len(),
        5
    );
    assert!(enough.check().is_ok());
    let short = budget(cost - 1);
    assert!(extract_non_image_stream_payloads(&short, bytes.as_bytes()).is_empty());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == cost)
    );
}

/// 非圧縮本文の個別上限もinclusiveであり、保存slotの前に不足を検出する。
#[test]
fn preview_plain_stream_individual_limit_is_inclusive() {
    for limit in [0, 1] {
        let target = DecodeBudget::new(&ParseLimits {
            max_decoded_stream_bytes: limit,
            ..Default::default()
        });
        let streams = extract_non_image_stream_payloads(&target, PDF);
        assert_eq!(streams.len(), limit as usize);
        assert_eq!(target.check().is_ok(), limit == 1);
    }
}
