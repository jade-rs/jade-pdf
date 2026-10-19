//! 個別用途を横断する確保容量の累積上限を検証する。
use super::*;
use proptest::prelude::*;
use std::io::Write;

/// 個別予算に達する前に共通容量の境界を検査する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_allocation_bytes: limit,
        ..Default::default()
    })
}

/// 共通の資源名と予約前に判定した新しい累積値を検査する。
fn exceeded(budget: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "total allocation bytes", limit: found_limit, actual: found_actual,
    }) if found_limit == limit && found_actual == actual));
    assert!(budget.try_vector_capacity::<u8>(0).is_none());
    assert!(budget.try_string_capacity(0).is_none());
    assert!(budget.check().is_ok());
    assert!(budget.is_stopped());
}

/// 別用途の配列・文字列を合算し、解放後も予約を戻さない。
#[test]
fn allocation_categories_share_an_inclusive_limit() {
    let budget = budget(20);
    drop(budget.metadata_vector::<u16>(3).unwrap());
    drop(budget.image_vector::<u8>(4).unwrap());
    drop(budget.font_vector::<u32>(2).unwrap());
    drop(budget.try_string_capacity(2).unwrap());
    assert!(budget.check().is_ok());
    assert!(budget.try_vector_capacity::<u8>(1).is_none());
    exceeded(&budget, 20, 21);
}

/// 予算ゼロでも無確保は成功し、ZSTの要素数をbyte数へ流用しない。
#[test]
fn empty_and_zero_sized_allocations_do_not_consume_bytes() {
    let budget = budget(0);
    assert_eq!(budget.try_vector_capacity::<u8>(0), Some(Vec::new()));
    assert_eq!(budget.try_string_capacity(0), Some(String::new()));
    assert!(budget.try_vector_capacity::<()>(usize::MAX).is_some());
    assert!(budget.image_vector::<u8>(1).is_none());
    exceeded(&budget, 0, 1);
}

/// 通常とFontの展開で、新容量全体を一度ずつ共通予算へ数える。
#[test]
fn decoder_reallocation_uses_the_shared_budget() {
    let bytes = vec![b'a'; 20000];
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    let compressed = encoder.finish().unwrap();
    let capacity = 8192 + 16384 + 32768;
    for font in [false, true] {
        let exact = budget(capacity);
        assert_eq!(
            decode_zlib_buffer(&compressed, &exact, font),
            Some(bytes.clone())
        );
        assert!(exact.try_vector_capacity::<u8>(1).is_none());
        exceeded(&exact, capacity, capacity + 1);
        let short = budget(capacity - 1);
        assert!(decode_zlib_buffer(&compressed, &short, font).is_none());
        exceeded(&short, capacity - 1, capacity);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 独立したbyte合算モデルで、用途と解放順に関係ない停止を検査する。
    #[test]
    fn mixed_allocations_follow_a_byte_sum(
        limit in 0u64..1024,
        operations in prop::collection::vec((0u8..4, 0usize..32), 0..40),
    ) {
        let budget = budget(limit);
        let mut sum = 0u64;
        let mut stopped = false;
        for (kind, count) in operations {
            let bytes = count as u64 * if kind == 2 { 2 } else { 1 };
            let expected = !stopped && sum + bytes <= limit;
            let accepted = match kind {
                0 => budget.metadata_vector::<u8>(count).is_some(),
                1 => budget.image_vector::<u8>(count).is_some(),
                2 => budget.font_vector::<u16>(count).is_some(),
                _ => budget.try_string_capacity(count).is_some(),
            };
            prop_assert_eq!(accepted, expected);
            if expected {
                sum += bytes;
            } else if !stopped {
                exceeded(&budget, limit, sum + bytes);
                stopped = true;
            }
        }
    }
}
