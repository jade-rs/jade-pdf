//! 未検査CMap streamの番号境界・字句・本文範囲・heap非確保を検証する。
use super::*;
use crate::{ParseLimits, test_allocations::measure};
use proptest::prelude::*;

/// 未圧縮本文は共通容量0でも入力を借用する。
fn budget() -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_allocation_bytes: 0,
        ..Default::default()
    })
}

/// 十進番号のsuffixを別objectと誤認しない。
#[test]
fn stream_lookup_number_boundary() {
    let bytes = b"11 0 obj << >> stream\nwrong\nendstream endobj\n1 0 obj << >> stream\nright\nendstream endobj";
    assert_eq!(
        extract_stream_payload_for_object(&budget(), bytes, 1)
            .unwrap()
            .as_ref(),
        b"right"
    );
    assert!(extract_stream_payload_for_object(&budget(), &bytes[..49], 1).is_none());
}

/// headerのPDF空白・コメントと最大u32番号を借用字句で読む。
#[test]
fn stream_lookup_header_trivia_and_maximum() {
    for header in ["1\t0\robj", "1% number\n0% generation\nobj", "1\0\x0c0 obj"] {
        let bytes = format!("{header} << >> stream\nx\nendstream endobj");
        assert_eq!(
            extract_stream_payload_for_object(&budget(), bytes.as_bytes(), 1)
                .unwrap()
                .as_ref(),
            b"x"
        );
    }
    let bytes = b"4294967295 0 obj << >> stream\nx\nendstream endobj";
    assert_eq!(
        extract_stream_payload_for_object(&budget(), bytes, u32::MAX)
            .unwrap()
            .as_ref(),
        b"x"
    );
}

/// コメント・文字列・name・dictionary内の偽headerは参照先にならない。
#[test]
fn stream_lookup_ignores_fake_headers() {
    for fake in [
        "% 1 0 obj << >> stream\n",
        "(1 0 obj << >> stream)\n",
        "/1 0 obj << >> stream\n",
        "<< /Fake [1 0 obj] >>\n",
        "1 0 object << >> stream\n",
    ] {
        let bytes = format!(
            "{fake}2 0 obj << >> stream\nwrong\nendstream endobj\n1 0 obj << /Note (stream) >> stream\nright\nendstream endobj"
        );
        assert_eq!(
            extract_stream_payload_for_object(&budget(), bytes.as_bytes(), 1)
                .unwrap()
                .as_ref(),
            b"right"
        );
    }
}

/// 非stream objectから次のobjectのstreamを借りない。
#[test]
fn stream_lookup_does_not_cross_endobj() {
    let bytes =
        b"1 0 obj << /Note (stream) >> endobj\n2 0 obj << >> stream\nwrong\nendstream endobj";
    assert!(extract_stream_payload_for_object(&budget(), bytes, 1).is_none());
}

/// 直接Lengthの本文内markerと末尾改行を保持し、非対象のbinary本文も飛ばす。
#[test]
fn stream_lookup_preserves_length_and_skips_binary() {
    for data in [
        b"x\r\n".as_slice(),
        b"endstream",
        b"endstream 1 0 obj << >> stream\nwrong",
    ] {
        let first = format!("2 0 obj << /Length {} >> stream\n", data.len());
        let mut bytes = first.into_bytes();
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(
            b"\nendstream endobj\n1 0 obj << /Length 3 >> stream\nx\r\n\nendstream endobj",
        );
        let result = extract_stream_payload_for_object(&budget(), &bytes, 1).unwrap();
        assert_eq!(result.as_ref(), b"x\r\n");
    }
}

/// 成功・未発見の探索ではheap要求がなく、停止後は本文を返さない。
#[test]
fn stream_lookup_has_no_heap_requests_and_respects_stop() {
    let bytes = b"1 0 obj << >> stream\nx\nendstream endobj";
    let allowed = budget();
    for number in [1, 3] {
        let (result, requests) =
            measure(|| extract_stream_payload_for_object(&allowed, bytes, number));
        assert_eq!(requests, (0, 0));
        assert_eq!(result.is_some(), number == 1);
    }
    let stopped = budget();
    assert!(stopped.try_vector_capacity::<u8>(1).is_none());
    let (result, requests) = measure(|| extract_stream_payload_for_object(&stopped, bytes, 1));
    assert!(result.is_none());
    assert_eq!(requests, (0, 0));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 独立した十進suffixモデルで別objectの本文を採用しない。
    #[test]
    fn stream_lookup_decimal_suffix_model(number in 0u32..10_000) {
        let bytes = format!("9{number} 0 obj << >> stream\nwrong\nendstream endobj\n{number} 0 obj << >> stream\nright\nendstream endobj");
        let allowed = budget();
        let (result, requests) = measure(|| extract_stream_payload_for_object(&allowed, bytes.as_bytes(), number));
        let result = result.unwrap();
        prop_assert_eq!(result.as_ref(), b"right".as_slice());
        prop_assert_eq!(requests, (0, 0));
        prop_assert!(allowed.check().is_ok());
    }
}
