//! 入力の再確保を用途横断の容量予算へ含める回帰。
use super::*;
use proptest::prelude::*;
use std::io::Cursor;

/// 共通確保容量だけを変更する。
fn limits(capacity: u64) -> ParseLimits {
    ParseLimits {
        max_total_allocation_bytes: capacity,
        ..Default::default()
    }
}

/// 入力とmetadataを合算し、再確保の新容量全体を数える。
#[test]
fn input_growth_shares_the_allocation_budget() {
    let bytes = vec![7; 20000];
    let total = 3 + 8192 + 16384 + 32768;
    let exact = limits(total);
    let budget = DecodeBudget::new(&exact);
    drop(budget.metadata_vector::<u8>(3).unwrap());
    assert_eq!(read(Cursor::new(&bytes), &exact, &budget).unwrap(), bytes);
    assert!(budget.try_vector_capacity::<u8>(1).is_none());
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "total allocation bytes", actual, ..
    }) if actual == total + 1));
    let short = limits(total - 1);
    let budget = DecodeBudget::new(&short);
    drop(budget.metadata_vector::<u8>(3).unwrap());
    let mut reader = Cursor::new(&bytes);
    assert!(
        matches!(read(&mut reader, &short, &budget), Err(ParseError::ResourceLimit {
        resource: "total allocation bytes", actual, ..
    }) if actual == total)
    );
    assert_eq!(reader.position(), bytes.len() as u64);
    assert!(budget.try_vector_capacity::<u8>(0).is_none());
}

/// 空入力は無確保で成功し、入力サイズ超過を確保失敗より先に診断する。
#[test]
fn empty_and_oversized_input_do_not_allocate() {
    let limits = ParseLimits {
        max_input_bytes: 0,
        ..limits(0)
    };
    let budget = DecodeBudget::new(&limits);
    assert!(read(Cursor::new([]), &limits, &budget).unwrap().is_empty());
    assert!(matches!(
        read(Cursor::new([1]), &limits, &budget),
        Err(ParseError::ResourceLimit {
            resource: "input bytes",
            limit: 0,
            actual: 1,
        })
    ));
    assert!(budget.try_vector_capacity::<u8>(0).is_some());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// chunk列の独立容量モデルと、同じ入力上限での境界を比較する。
    #[test]
    fn input_capacity_follows_chunk_growth(length in 0usize..40000) {
        let mut capacity = 0usize;
        let mut size = 0usize;
        let mut total = 0u64;
        while size < length {
            size += (length - size).min(8192);
            if size > capacity {
                capacity = (capacity * 2).max(size).min(length);
                total += capacity as u64;
            }
        }
        let source = vec![9; length];
        let exact = ParseLimits { max_input_bytes: length as u64, ..limits(total) };
        prop_assert_eq!(read(Cursor::new(&source), &exact, &DecodeBudget::new(&exact)).unwrap(), source.clone());
        if total > 0 {
            let short = ParseLimits { max_total_allocation_bytes: total - 1, ..exact };
            prop_assert!(matches!(read(Cursor::new(&source), &short, &DecodeBudget::new(&short)), Err(ParseError::ResourceLimit {
                resource: "total allocation bytes", actual, ..
            }) if actual == total), "input allocation boundary");
        }
    }
}
