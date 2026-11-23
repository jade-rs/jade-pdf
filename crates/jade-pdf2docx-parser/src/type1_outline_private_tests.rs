//! 自分で作った二進辞書だけで、境界・重複・実行不能条件を固定する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 検証対象のbinaryを一つずつRDに渡す、所有の静的な辞書。
fn private(glyph: &[u8], subr: &[u8], separator: &[u8]) -> Vec<u8> {
    let mut bytes = b"dup /Private 8 dict dup begin\n/RD{string currentfile exch readstring pop}executeonly def\n/ND{noaccess def}executeonly def /NP{noaccess put}executeonly def\n/lenIV 4 def /Subrs 1 array\ndup 0 ".to_vec();
    bytes.extend_from_slice(subr.len().to_string().as_bytes());
    bytes.extend_from_slice(b" RD");
    bytes.extend_from_slice(separator);
    bytes.extend_from_slice(subr);
    bytes.extend_from_slice(
        b" NP\nND end readonly put dup /CharStrings 1 dict dup begin\n/.notdef ",
    );
    bytes.extend_from_slice(glyph.len().to_string().as_bytes());
    bytes.extend_from_slice(b" RD");
    bytes.extend_from_slice(separator);
    bytes.extend_from_slice(glyph);
    bytes.extend_from_slice(
        b" ND\nend readonly put dup /FontName get exch definefont pop mark currentfile closefile\n",
    );
    bytes
}

/// delimiter・comment・NULを含むbinaryを、tokenとして読み直さない。
#[test]
fn binary_payload_and_charstring_prefix_survive() {
    let source = private(b"%{}\0/CharStrings\n", b"ND\r\n()", b"\r\n");
    let parsed = parse(&DecodeBudget::default(), &source).unwrap();
    assert_eq!(parsed.prefix, Some(4));
    assert_eq!(parsed.subrs, vec![Some(b"ND\r\n()".as_slice())]);
    assert_eq!(parsed.glyphs.len(), 1);
    assert_eq!(parsed.glyphs[0].name, b".notdef");
    assert_eq!(parsed.glyphs[0].bytes, b"%{}\0/CharStrings\n");
}

/// 容量予約を前置し、未知のRD実装や動的glyph辞書を推測しない。
#[test]
fn unsafe_definitions_and_storage_limit_are_rejected() {
    let source = private(b"abc", b"def", b" ");
    let target = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(parse(&target, &source).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            ..
        })
    ));
    let text = String::from_utf8(source).unwrap();
    for changed in [
        text.replace("readstring pop", "readstring pop run"),
        text.replace(
            "/CharStrings 1 dict",
            "/CharStrings 1 dict dup /Other begin",
        ),
        text.replace("/.notdef 3 RD", "/.notdef 999999999 RD"),
        text.replace("/lenIV 4 def", "/lenIV -2 def"),
        text.replace("/lenIV 4 def", "/lenIV 4 def /lenIV 4 def"),
        text.replace("/lenIV 4 def", "/lenIV 4 def /OtherSubrs [] def"),
        text.replace("/Subrs 1 array", "/Subrs 999999999 array"),
        text.replace("closefile", "closefile run"),
    ] {
        assert!(parse(&DecodeBudget::default(), changed.as_bytes()).is_none());
    }
}

/// 平文・prefixなし・既定prefixを区別し、未初期化slotと重複を見逃さない。
#[test]
fn explicit_prefix_and_dictionary_identity_are_not_inferred() {
    let source = String::from_utf8(private(b"abc", b"def", b" ")).unwrap();
    for (definition, expected) in [
        ("", Some(4)),
        ("/lenIV -1 def", None),
        ("/lenIV 0 def", Some(0)),
        ("/lenIV 64 def", Some(64)),
    ] {
        let changed = source.replace("/lenIV 4 def", definition);
        assert_eq!(
            parse(&DecodeBudget::default(), changed.as_bytes())
                .unwrap()
                .prefix,
            expected
        );
    }
    let sparse = source.replace("/Subrs 1 array", "/Subrs 2 array");
    assert_eq!(
        parse(&DecodeBudget::default(), sparse.as_bytes())
            .unwrap()
            .subrs,
        vec![Some(b"def".as_slice()), None]
    );
    for changed in [
        source.replace("def NP", "def NP dup 0 3 RD def NP"),
        source
            .replace("/CharStrings 1 dict", "/CharStrings 2 dict")
            .replace("abc ND", "abc ND /.notdef 3 RD abc ND"),
        source.replace("/.notdef", "/A"),
    ] {
        assert!(parse(&DecodeBudget::default(), changed.as_bytes()).is_none());
    }
}

/// TeX形式の入れ子と既知hintは許し、値の演算・上書き・過大配列を拒否する。
#[test]
fn known_literal_hints_and_nested_dictionary_are_bounded() {
    let source = String::from_utf8(private(b"abc", b"def", b" ")).unwrap();
    let known = "/MinFeature{16 16}ND /password 5839 def /BlueValues[-10 0 700 710]ND /OtherBlues[]ND /StdHW[30]ND /StdVW[70]ND /StemSnapH[25 30]ND /BlueScale .04379 def /BlueShift 7 def /BlueFuzz 1 def /ForceBold false def";
    let changed = source.replace("/lenIV 4 def", known);
    assert!(parse(&DecodeBudget::default(), changed.as_bytes()).is_some());
    let nested = changed
        .replace(
            "ND end readonly put dup /CharStrings",
            "ND 2 index /CharStrings",
        )
        .replace(
            "end readonly put dup /FontName",
            "end end readonly put put dup /FontName",
        );
    assert_eq!(
        parse(&DecodeBudget::default(), nested.as_bytes())
            .unwrap()
            .glyphs[0]
            .bytes,
        b"abc"
    );
    for invalid in [
        "/BlueValues[1 2 add]ND".to_string(),
        "/BlueValues[1 {2} 3]ND".to_string(),
        "/BlueScale NaN def".to_string(),
        "/BlueScale 1e500 def".to_string(),
        "/ForceBold 1 def".to_string(),
        "/MinFeature{16 16 run}ND".to_string(),
        "/BlueValues[]ND /BlueValues[]ND".to_string(),
        "/RD{run}executeonly def".to_string(),
        format!("/BlueValues[{}]ND", "1 ".repeat(33)),
    ] {
        let changed = source.replace("/lenIV 4 def", &invalid);
        assert!(
            parse(&DecodeBudget::default(), changed.as_bytes()).is_none(),
            "{invalid}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// binary中の字句に依存せず、宣言長ちょうどの二つのprogramを保持する。
    #[test]
    fn arbitrary_binary_is_length_delimited(glyph in proptest::collection::vec(any::<u8>(),0..512),subr in proptest::collection::vec(any::<u8>(),0..512),crlf in any::<bool>()) {
        let bytes = private(&glyph,&subr,if crlf { b"\r\n" } else { b" " });
        let parsed = parse(&DecodeBudget::default(), &bytes).unwrap();
        prop_assert_eq!(parsed.glyphs[0].bytes,glyph.as_slice());
        prop_assert_eq!(parsed.subrs[0],Some(subr.as_slice()));
    }

    /// 配列値を計算・描画へ転用せず、固定の個数上限だけを一貫して適用する。
    #[test]
    fn hint_literal_count_has_a_fixed_boundary(values in proptest::collection::vec(-32000_i32..32000,0..36)) {
        let source=String::from_utf8(private(b"abc",b"def",b" ")).unwrap();
        let hint=format!("/BlueValues[{}]ND",values.iter().map(i32::to_string).collect::<Vec<_>>().join(" "));
        let changed=source.replace("/lenIV 4 def",&hint);
        prop_assert_eq!(parse(&DecodeBudget::default(),changed.as_bytes()).is_some(),values.len()<=32);
    }
}
