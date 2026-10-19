//! Font 用 zlib 配列の累積容量と共通展開予算を検証する。
use super::*;
use std::io::Write;

/// 任意の byte 列を zlib stream にする。
fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// 空の展開は無確保で成功するが、非空展開は保存予算ゼロで停止する。
#[test]
fn font_decoder_checks_capacity_before_first_append() {
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert_eq!(
        decode_zlib_font(&compressed(b""), &budget),
        Some(Vec::new())
    );
    assert!(decode_zlib_font(&compressed(b"x"), &budget).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            actual: 1,
            ..
        })
    ));
    assert!(decode_zlib_font(&compressed(b""), &budget).is_none());
}

/// 拡大時は旧配列との差分だけでなく、新配列の全容量を累積へ予約する。
#[test]
fn font_decoder_counts_full_reallocation_and_repeated_decode() {
    let bytes = vec![b'a'; 20000];
    let compressed = compressed(&bytes);
    let capacity = 8192 + 16384 + 32768;
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: capacity - 1,
        ..Default::default()
    });
    assert!(decode_zlib_font(&compressed, &budget).is_none());
    assert!(
        matches!(budget.check(), Err(ParseError::ResourceLimit { resource: "embedded font bytes", actual, .. }) if actual == capacity)
    );
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: capacity,
        ..Default::default()
    });
    assert_eq!(decode_zlib_font(&compressed, &budget), Some(bytes));
    assert!(budget.check().is_ok());
    assert!(decode_zlib_font(&compressed, &budget).is_none());
}

/// Font 用展開でも、既存の stream/cumulative 展開上限を先に検査する。
#[test]
fn font_decoder_preserves_common_decode_limits() {
    for limits in [
        ParseLimits {
            max_decoded_stream_bytes: 1,
            ..Default::default()
        },
        ParseLimits {
            max_total_decoded_bytes: 1,
            ..Default::default()
        },
    ] {
        let budget = DecodeBudget::new(&limits);
        assert!(decode_zlib_font(&compressed(b"xx"), &budget).is_none());
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "decoded stream bytes" | "total decoded bytes",
                actual: 2,
                ..
            })
        ));
    }
}

/// 容量 overflow による fallible allocation 失敗でも、停止を保持する。
#[test]
fn font_vector_allocation_failure_is_retained() {
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: u64::MAX,
        ..Default::default()
    });
    assert!(budget.font_vector::<u8>(usize::MAX).is_none());
    assert!(
        matches!(budget.check(), Err(ParseError::Io(error)) if error.kind() == std::io::ErrorKind::OutOfMemory)
    );
    assert!(budget.font_vector::<u8>(0).is_none());
}
