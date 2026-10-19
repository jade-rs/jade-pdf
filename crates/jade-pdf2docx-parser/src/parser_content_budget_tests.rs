//! 未圧縮本文と圧縮本文が同じ保存予算を消費することを検証する。
use super::*;
use proptest::prelude::*;
use std::io::Write;

/// 本文以外の予算を既定値に保ち、本文予算だけ指定する。
fn budget(per_stream: u64, total: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_decoded_stream_bytes: per_stream,
        max_total_decoded_bytes: total,
        ..Default::default()
    })
}

/// 未圧縮 Form の保存は一度だけ課金し、後続の Contents と累積する。
#[test]
fn plain_form_and_contents_share_copy_budget() {
    let budget = budget(3, 6);
    let pdf = b"1 0 obj << /Length 3 >> stream\nabc\nendstream endobj";
    assert_eq!(
        form_stream_payload(&budget, pdf, 1, &StreamOffsets::default()),
        Some(b"abc".to_vec())
    );
    assert_eq!(
        decode_content_stream_payload(&budget, b"<< >>", b"def"),
        Some(b"def".to_vec())
    );
    assert!(budget.check().is_ok());
    assert_eq!(decode_content_stream_payload(&budget, b"<< >>", b"g"), None);
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "total decoded bytes",
            limit: 6,
            actual: 7,
        })
    ));
}

/// 最初の失敗を公開済みでも、未圧縮本文の保存を再開しない。
#[test]
fn stopped_plain_content_copy_does_not_resume() {
    let budget = budget(0, 0);
    assert_eq!(decode_content_stream_payload(&budget, b"<< >>", b"a"), None);
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "decoded stream bytes",
            limit: 0,
            actual: 1,
        })
    ));
    assert_eq!(decode_content_stream_payload(&budget, b"<< >>", b""), None);
    assert!(budget.is_stopped());
    assert!(budget.check().is_ok());
}

proptest! {
    /// 圧縮・未圧縮の順序と byte 値によらず、共有累積予算の境界を守る。
    #[test]
    fn mixed_streams_preserve_bytes_and_share_budget(
        plain in prop::collection::vec(any::<u8>(), 0..65),
        compressed in prop::collection::vec(any::<u8>(), 0..65),
        plain_first in any::<bool>(),
    ) {
        let total = (plain.len() + compressed.len()) as u64;
        let budget = budget(65, total);
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&compressed).unwrap();
        let encoded = encoder.finish().unwrap();
        let inputs = if plain_first {
            [(b"<< >>".as_slice(), plain.as_slice(), plain.as_slice()),
             (b"<< /Filter /FlateDecode >>".as_slice(), encoded.as_slice(), compressed.as_slice())]
        } else {
            [(b"<< /Filter /FlateDecode >>".as_slice(), encoded.as_slice(), compressed.as_slice()),
             (b"<< >>".as_slice(), plain.as_slice(), plain.as_slice())]
        };
        for (dictionary, data, expected) in inputs {
            let decoded = decode_content_stream_payload(&budget, dictionary, data);
            prop_assert_eq!(decoded.as_deref(), Some(expected));
        }
        prop_assert!(budget.check().is_ok());
        prop_assert_eq!(decode_content_stream_payload(&budget, b"<< >>", b"x"), None);
        let exhausted = matches!(budget.check(), Err(ParseError::ResourceLimit {
            resource: "total decoded bytes", limit, actual,
        }) if limit == total && actual == total + 1);
        prop_assert!(exhausted);
    }
}
