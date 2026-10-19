//! Rootなしstream集約の保存容量・再作成・停止時の部分結果破棄を固定する。
use super::*;
use proptest::prelude::*;
use std::io::Write;

/// 本文の展開予算とは別に、集約slotの累積容量を指定する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// object番号順と本文の保存slot以外の確保はテスト準備へ分離する。
fn pdf(count: usize) -> Vec<u8> {
    let mut pdf = Vec::new();
    for number in (1..=count).rev() {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&[(number % 128) as u8]).unwrap();
        let compressed = encoder.finish().unwrap();
        write!(
            pdf,
            "\n{number} 0 obj << /Filter /FlateDecode /Length {} >> stream\n",
            compressed.len()
        )
        .unwrap();
        pdf.extend_from_slice(&compressed);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }
    pdf
}

/// hash mapの全拡張slotと完成した番号付き配列の容量をモデル化する。
fn cost(count: usize) -> u64 {
    let mut capacity = 0;
    let mut slots = 0;
    for used in 0..count {
        if used >= capacity / 2 {
            capacity = (capacity * 2).max(4);
            slots += capacity;
        }
    }
    (slots * std::mem::size_of::<Option<(u32, Vec<u8>)>>()
        + count * std::mem::size_of::<(usize, Vec<u8>)>()) as u64
}

/// 初回・倍増・完成配列の全容量がinclusiveであり、一byte不足なら全体を破棄する。
#[test]
fn rootless_stream_capacity_is_inclusive() {
    for count in [1, 3, 5] {
        let bytes = pdf(count);
        let expected = cost(count);
        let enough = budget(expected);
        assert_eq!(extract_flate_streams(&enough, &bytes).len(), count);
        assert!(enough.check().is_ok());
        let short = budget(expected - 1);
        assert!(extract_flate_streams(&short, &bytes).is_empty());
        assert!(matches!(short.check(), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", actual, ..
        }) if actual == expected));
    }
}

/// 解放した保存領域も返却せず、エラー取得後にも停止状態を維持する。
#[test]
fn rootless_stream_recreation_is_cumulative() {
    let bytes = pdf(1);
    let expected = cost(1);
    let target = budget(2 * expected - 1);
    assert_eq!(extract_flate_streams(&target, &bytes).len(), 1);
    assert!(extract_flate_streams(&target, &bytes).is_empty());
    assert!(
        matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == 2 * expected)
    );
    assert!(extract_flate_streams(&target, &bytes).is_empty());
    assert!(target.is_stopped());
    assert!(target.check().is_ok());
}

/// 後続本文の展開失敗も先行する完成streamを返さない。
#[test]
fn rootless_later_decode_failure_discards_prior_streams() {
    let target = DecodeBudget::new(&ParseLimits {
        max_total_decoded_bytes: 1,
        ..Default::default()
    });
    assert!(extract_flate_streams(&target, &pdf(2)).is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "total decoded bytes",
            limit: 1,
            actual: 2,
        })
    ));
    assert!(extract_flate_streams(&target, &pdf(1)).is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 件数に応じたhash占有率・配列容量の累積モデルを守る。
    #[test]
    fn rootless_stream_capacity_matches_model(count in 1usize..17, repeats in 1usize..4) {
        let bytes = pdf(count);
        let expected = cost(count) * repeats as u64;
        let enough = budget(expected);
        for _ in 0..repeats {
            prop_assert_eq!(extract_flate_streams(&enough, &bytes).len(), count);
        }
        prop_assert!(enough.check().is_ok());
        let short = budget(expected - 1);
        for _ in 1..repeats {
            prop_assert_eq!(extract_flate_streams(&short, &bytes).len(), count);
        }
        prop_assert!(extract_flate_streams(&short, &bytes).is_empty());
        let exhausted = matches!(short.check(), Err(ParseError::ResourceLimit {actual, ..}) if actual == expected);
        prop_assert!(exhausted);
    }
}

/// zlib末尾のLF byteを区切りの改行として除去しない。
#[test]
fn rootless_binary_stream_uses_declared_length() {
    let target = budget(cost(9));
    let streams = extract_flate_streams(&target, &pdf(9));
    assert_eq!(
        streams,
        (0usize..9)
            .map(|page| (page, vec![(page + 1) as u8]))
            .collect::<Vec<_>>()
    );
    assert!(target.check().is_ok());
}

/// xrefなし重複番号は先行本文を採用し、捨てる本文を再展開しない。
#[test]
fn rootless_duplicates_preserve_first_payload_without_decoding() {
    let mut bytes = pdf(2);
    let mut later = pdf(3);
    later[1] = b'1';
    bytes.extend_from_slice(&later);
    let target = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: cost(2),
        max_total_decoded_bytes: 2,
        ..Default::default()
    });
    assert_eq!(
        extract_flate_streams(&target, &bytes),
        vec![(0, vec![1]), (1, vec![2])]
    );
    assert!(target.check().is_ok());
}
