//! 所有のPostScript文字列でescape・改行・全byte・累積確保を検査する。
use super::*;
use crate::{ParseCancellation, ParseError, ParseLimits};
use proptest::prelude::*;

/// octal・標準escape・未知escape・括弧のnestを、ASCII推測せずbyteとして読む。
#[test]
fn escapes_and_line_endings_preserve_literal_semantics() {
    for (raw, expected) in [
        (
            br"Copyright \050c\051 Owner".as_slice(),
            b"Copyright (c) Owner".as_slice(),
        ),
        (
            br"A\)B\\C\n\r\t\b\f\q".as_slice(),
            b"A)B\\C\n\r\t\x08\x0cq".as_slice(),
        ),
        (b"A\\\r\nB\r\nC\rD\nE".as_slice(), b"AB\nC\nD\nE".as_slice()),
        (b"nested (text)".as_slice(), b"nested (text)".as_slice()),
        (
            br"\377\400\777\12Z".as_slice(),
            b"\xff\x00\xff\nZ".as_slice(),
        ),
    ] {
        assert_eq!(decode(&DecodeBudget::default(), raw).unwrap(), expected);
    }
}

/// decoded長だけを予約し、繰返し・不足・取消の状態を失わない。
#[test]
fn output_length_is_exact_and_cumulative() {
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 2,
        ..Default::default()
    });
    assert_eq!(decode(&budget, br"\101\102"), Some(b"AB".to_vec()));
    assert!(decode(&budget, br"\101\102").is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            actual: 4,
            ..
        })
    ));
    let token = ParseCancellation::new();
    token.cancel();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(decode(&budget, b"owned").is_none());
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
}

/// 不正な本文は保存前に失敗し、文字列の途中だけを返さない。
#[test]
fn malformed_or_oversized_body_does_not_allocate() {
    for raw in [b"(".as_slice(), b")", b"A\\", b"A)B", b"A(B"] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: 0,
            ..Default::default()
        });
        assert!(decode(&budget, raw).is_none());
        assert!(budget.check().is_ok());
    }
    assert!(decode(&DecodeBudget::default(), &vec![b'A'; 65536]).is_none());
    let empty = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert_eq!(decode(&empty, b""), Some(vec![]));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意byte列を三桁octalで表しても、literal本文と既存PDF文字列の両経路で元に戻る。
    #[test]
    fn every_byte_roundtrips_through_shared_escape_rules(bytes in prop::collection::vec(any::<u8>(),0..256)) {
        let raw=bytes.iter().map(|b|format!("\\{b:03o}")).collect::<String>();
        prop_assert_eq!(decode(&DecodeBudget::default(),raw.as_bytes()).unwrap(),bytes.clone());
        let delimited=format!("({raw})");let mut output=Vec::new();
        prop_assert!(crate::pdf_binary_string::visit_string(delimited.as_bytes(),|b|{output.push(b);Some(())}).is_some());
        prop_assert_eq!(output,bytes);
    }
}
