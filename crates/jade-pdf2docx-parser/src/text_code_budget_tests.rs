//! Font幅計算に用いるcode byte復号の容量境界と改行を固定する。
use super::*;
use crate::{ParseError, ParseLimits, font_widths::Metrics, page_attributes::Objects};
use proptest::prelude::*;

/// code byteだけの累積容量を指定する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 幅表の準備はテスト対象のcode復号予算から分離する。
fn metrics(dictionary: &[u8]) -> Metrics {
    Metrics::read(&Objects::new(), dictionary, &DecodeBudget::default(), &[])
}

/// literal/hexの復号長だけを予約し、hexの空白・奇数桁に中間配列を作らない。
#[test]
fn code_byte_capacity_is_inclusive() {
    for (raw, expected) in [
        (b"(a\\000b\\\r\nc)".as_slice(), b"a\0bc".as_slice()),
        (b"< 41\t4 >".as_slice(), b"A@".as_slice()),
        (b"(a\r\nb\rc)".as_slice(), b"a\nb\nc".as_slice()),
    ] {
        let cost = expected.len() as u64;
        let enough = budget(cost);
        assert_eq!(pdf_string_bytes(&enough, raw).as_deref(), Some(expected));
        assert!(enough.check().is_ok());
        let short = budget(cost - 1);
        assert!(pdf_string_bytes(&short, raw).is_none());
        assert!(
            matches!(short.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == cost)
        );
    }
}

/// simple/CIDと未対応幅のspacing推定も同じcode予算を使う。
#[test]
fn font_layouts_share_code_capacity_and_stop() {
    let courier = metrics(b"<< /Subtype /Type1 /BaseFont /Courier >>");
    let unknown = metrics(b"<< /Subtype /Type1 /BaseFont /Unknown >>");
    let cid = metrics(b"<< /Subtype /Type0 /Encoding /Identity-H /DescendantFonts [<< /Subtype /CIDFontType2 >>] >>");
    for kind in 0..3 {
        let cost = if kind == 2 { 4 } else { 2 };
        for limit in [cost - 1, cost] {
            let target = budget(limit);
            let layout = match kind {
                0 => courier.layout(&target, b"(AB)", 10., 1., 3.),
                1 => unknown.estimated_layout(&target, b"(AB)", 12., 1., 3.),
                _ => cid.layout(&target, b"<00410042>", 10., 1., 3.),
            };
            if limit == cost {
                assert_eq!(layout.unwrap().advance, if kind == 2 { 22. } else { 14. });
                assert!(target.check().is_ok());
            } else {
                assert!(layout.is_none());
                assert!(
                    matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost)
                );
                assert!(courier.layout(&target, b"(A)", 10., 0., 0.).is_none());
                assert!(
                    unknown
                        .estimated_layout(&target, b"(A)", 6., 0., 0.)
                        .is_none()
                );
                assert!(cid.layout(&target, b"<0041>", 10., 0., 0.).is_none());
                assert!(target.is_stopped());
            }
        }
    }
}

/// raw CRLFは一つのLF codeへ正規化し、glyph数とspacingを揃える。
#[test]
fn font_spacing_uses_normalized_literal_line_endings() {
    let courier = metrics(b"<< /Subtype /Type1 /BaseFont /Courier >>");
    let target = budget(5);
    let layout = courier
        .layout(&target, b"(a\r\nb\rc)", 10., 1., 0.)
        .unwrap();
    assert_eq!(layout.advance, 35.);
    assert_eq!(layout.right, 34.);
    assert!(target.check().is_ok());
}

/// 不正な文字列の途中byteを保存せず、容量も消費しない。
#[test]
fn invalid_code_operands_do_not_consume_capacity() {
    let target = budget(1);
    for raw in [b"<41XZ>".as_slice(), b"(x) trailing", b"(unclosed", b"<<>>"] {
        assert!(pdf_string_bytes(&target, raw).is_none());
        assert!(target.check().is_ok());
    }
    assert_eq!(
        pdf_string_bytes(&target, b"(x)").as_deref(),
        Some(b"x".as_slice())
    );
    assert!(target.check().is_ok());
    assert!(pdf_string_bytes(&target, b"(y)").is_none());
    assert!(target.check().is_err());
    assert!(pdf_string_bytes(&target, b"<>").is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 反復するliteral/hex復号の累積容量はencoded byte数のモデルと一致する。
    #[test]
    fn repeated_code_storage_matches_model(length in 1usize..128, repeats in 1usize..5, hex in any::<bool>(), short in any::<bool>()) {
        let raw = if hex { format!("<{}>", "41".repeat(length)) } else { format!("({})", "A".repeat(length)) };
        let cost = (length * repeats) as u64;
        let target = budget(cost - u64::from(short));
        for call in 0..repeats {
            let result = pdf_string_bytes(&target, raw.as_bytes());
            if short && call + 1 == repeats {
                prop_assert!(result.is_none());
                prop_assert!(target.check().is_err());
            } else {
                prop_assert_eq!(result, Some(vec![b'A'; length]));
                prop_assert!(target.check().is_ok());
            }
        }
        prop_assert_eq!(target.is_stopped(), short);
    }
}
