//! AGLの独立した既知値・Unicode境界・未知名・協調停止を検査する。
use super::*;
use proptest::prelude::*;

/// 所有の小さな期待値と比較するため、通知されたscalarを一時Stringへ集める。
fn decode(name: &str, zapf: bool) -> (String, bool) {
    let mut text = String::new();
    let complete = visit(&DecodeBudget::default(), name.as_bytes(), zapf, |scalar| {
        text.push(scalar);
        Some(())
    })
    .unwrap();
    (text, complete)
}

/// AGLの既知名・複数scalar・BMP/nonBMPとsuffixを標準の意味で復元する。
#[test]
fn known_names_and_compounds_preserve_unicode() {
    for (name, expected) in [
        ("A", "A"),
        ("Omega", "\u{2126}"),
        ("ffi", "\u{fb03}"),
        ("dalethatafpatah", "\u{05d3}\u{05b2}"),
        (
            "Lcommaaccent_uni20AC0308_u1040C.alternate",
            "\u{013b}\u{20ac}\u{0308}\u{1040c}",
        ),
        ("uni00410042", "AB"),
        ("u10FFFF", "\u{10ffff}"),
    ] {
        assert_eq!(decode(name, false), (expected.into(), true), "{name}");
    }
}

/// surrogate・範囲外・小文字hex・独自名を、見かけ上のUnicodeに推測しない。
#[test]
fn invalid_and_opaque_names_are_unverified() {
    for name in [
        "G41",
        ".notdef",
        "uD800",
        "u110000",
        "uniD801DC0C",
        "uni0041D800",
        "uni20ac",
        "u1f600",
        "u123",
        "uni",
        "日本語",
    ] {
        assert_eq!(decode(name, false), (String::new(), false), "{name}");
    }
}

/// 不明componentは空にする標準動作を守り、既知部分だけでも完全確認済みにはしない。
#[test]
fn unknown_component_does_not_erase_known_components() {
    assert_eq!(decode("A_UnknownGlyph_B.alt", false), ("AB".into(), false));
}

/// Zapf固有の名は、そのfontであることを確認した場合だけ利用する。
#[test]
fn zapf_names_do_not_leak_into_other_fonts() {
    assert_eq!(decode("a1", true), ("\u{2701}".into(), true));
    assert_eq!(decode("a1", false), (String::new(), false));
}

/// 開始前と通知途中のキャンセルでは、完全確認済みの結果を返さない。
#[test]
fn cancellation_remains_terminal() {
    let token = crate::ParseCancellation::new();
    let budget = DecodeBudget::with_cancellation(&crate::ParseLimits::default(), Some(&token));
    let result = visit(&budget, b"uni00410042", false, |_| {
        token.cancel();
        Some(())
    });
    assert!(result.is_none());
    assert!(matches!(budget.check(), Err(crate::ParseError::Cancelled)));
    assert!(visit(&budget, b"G41", false, |_| Some(())).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 全Unicode scalarのu形式と、そのsuffixは元の文字へ復元できる。
    #[test]
    fn scalar_name_roundtrip(scalar in any::<char>()) {
        let name = format!("u{:04X}.owned", scalar as u32);
        prop_assert_eq!(decode(&name, false), (scalar.to_string(), true));
    }
}
