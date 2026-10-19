//! 構造検査中のstream一覧とcontainer stackの累積要求容量を検証する。
use super::*;
use crate::decode::DecodeBudget;
use proptest::prelude::*;

/// metadata容量だけを指定して、構造検査と同じ共有予算を作る。
fn budget(limit: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit as u64,
        ..Default::default()
    })
}

/// 任意の個数の未圧縮streamを持つxrefなしのPDFを作る。
fn stream_pdf(count: usize) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    for number in 1..=count {
        bytes.extend_from_slice(
            format!("{number} 0 obj << /Length 1 >> stream\na\nendstream endobj\n").as_bytes(),
        );
    }
    bytes
}

/// stackとstream一覧の初回4要素・倍増配列の全容量を独立に計算する。
fn cumulative_slots(length: usize) -> usize {
    let mut capacity = 0;
    let mut total = 0;
    while capacity < length {
        capacity = (capacity * 2).max(4);
        total += capacity;
    }
    total
}

/// 共有予算を使用して構造検査を実行する。
fn validate<'a>(bytes: &'a [u8], budget: &DecodeBudget) -> Result<Vec<StreamSpan<'a>>, ParseError> {
    validate_structure(
        std::path::Path::new("structure-budget.pdf"),
        bytes,
        9,
        &ParseLimits::default(),
        budget,
    )
}

/// container stackは容量0の時点で確保せず、公開可能な予算エラーを返す。
#[test]
fn container_stack_requires_metadata_capacity() {
    assert!(
        matches!(validate(b"%PDF-1.7\n1 0 obj << >> endobj", &budget(0)),
        Err(ParseError::ResourceLimit {resource: "metadata workspace bytes", limit: 0, actual})
        if actual == (4 * std::mem::size_of::<Frame>()) as u64)
    );
}

/// containerを閉じて再利用しても、stream一覧の倍増時は新配列全体を加算する。
#[test]
fn stream_list_growth_is_cumulative_and_inclusive() {
    let bytes = stream_pdf(5);
    let required = 4 * std::mem::size_of::<Frame>() + 12 * std::mem::size_of::<StreamSpan<'_>>();
    for limit in [required - 1, required] {
        let result = validate(&bytes, &budget(limit));
        if limit == required {
            let streams = result.unwrap();
            assert_eq!(streams.len(), 5);
            assert!(streams.iter().all(|stream| stream.payload == b"a"));
            assert_eq!(streams[4].object_number, Some(5));
        } else {
            assert!(matches!(result, Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            }) if actual == required as u64));
        }
    }
}

/// 解放した検査結果とstackの容量も、次回検査へ返却しない。
#[test]
fn repeated_structure_validation_is_cumulative() {
    let bytes = stream_pdf(1);
    let one = 4 * (std::mem::size_of::<Frame>() + std::mem::size_of::<StreamSpan<'_>>());
    for limit in [one * 2 - 1, one * 2] {
        let budget = budget(limit);
        drop(validate(&bytes, &budget).unwrap());
        let second = validate(&bytes, &budget);
        assert_eq!(second.is_ok(), limit == one * 2);
        if let Err(error) = second {
            assert!(matches!(error, ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            } if actual == (one * 2) as u64));
        }
    }
}

/// 予算エラーを消費した後は、配列を必要としない入力も再検査しない。
#[test]
fn consumed_failure_does_not_restart_structure_validation() {
    let budget = budget(0);
    assert!(budget.metadata_vector::<u8>(1).is_none());
    assert!(budget.check().is_err());
    assert!(validate(b"%PDF-1.7\n1 0 obj 0 endobj", &budget).is_err());
    assert!(budget.is_stopped());
}

proptest! {
    /// stack深度とstream件数の独立モデルで、全確保容量の境界と借用本文を固定する。
    #[test]
    fn structure_capacity_matches_growth_model(depth in 1_usize..33, count in 1_usize..33) {
        let mut bytes = b"%PDF-1.7\n100 0 obj ".to_vec();
        bytes.extend(std::iter::repeat_n(b'[', depth));
        bytes.push(b'0');
        bytes.extend(std::iter::repeat_n(b']', depth));
        bytes.extend_from_slice(b" endobj\n");
        bytes.extend_from_slice(&stream_pdf(count)[9..]);
        let required = cumulative_slots(depth) * std::mem::size_of::<Frame>()
            + cumulative_slots(count) * std::mem::size_of::<StreamSpan<'_>>();
        let exact = budget(required);
        let streams = validate(&bytes, &exact).unwrap();
        prop_assert!(exact.check().is_ok());
        prop_assert_eq!(streams.len(), count);
        for stream in streams {
            prop_assert_eq!(stream.payload, b"a");
            let start = stream.payload.as_ptr() as usize - bytes.as_ptr() as usize;
            prop_assert!(start < bytes.len());
        }
        prop_assert!(matches!(validate(&bytes, &budget(required - 1)),
            Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. })
            if actual == required as u64), "構造検査の容量不足を返さなかった");
    }
}
