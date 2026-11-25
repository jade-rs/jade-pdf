//! Type1 Encodingの静的構文・上書き・偽定義・予算境界を検査する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 所有のcleartextだけを持つ、字句解析用font headerを作る。
fn header(body: &str) -> String {
    format!(
        "%!PS-AdobeFont-1.0: Owned 1.0\n11 dict begin\n{body}\ncurrentdict end\ncurrentfile eexec\n"
    )
}

/// 独自表の初期fillと、複数回のdup/putは実行順で解決する。
#[test]
fn static_custom_assignments_preserve_last_value() {
    let body = header(
        "/Encoding 256 array 0 1 255 {1 index exch /.notdef put} for dup 65 /A put dup 66 /B put dup 65 /C put readonly def",
    );
    let budget = DecodeBudget::default();
    let table = parse(&budget, body.as_bytes()).unwrap();
    assert_eq!(table.base, Base::Custom);
    assert_eq!(table.slots.len(), 256);
    assert_eq!(table.slots[65], Some(b"C".as_slice()));
    assert_eq!(table.slots[66], Some(b"B".as_slice()));
    assert_eq!(table.slots[64], None);
    assert!(budget.check().is_ok());
}

/// 既知の名前付き初期表と、その複製上の上書きを区別する。
#[test]
fn named_bases_and_copied_overrides_are_distinct() {
    for (body, expected, name) in [
        ("/Encoding StandardEncoding def", Base::Standard, None),
        ("/Encoding ISOLatin1Encoding def", Base::Latin1, None),
        (
            "/Encoding StandardEncoding 256 array copy dup 65 /B put readonly def",
            Base::Standard,
            Some(b"B".as_slice()),
        ),
    ] {
        let source = header(body);
        let table = parse(&DecodeBudget::default(), source.as_bytes()).unwrap();
        assert_eq!(table.base, expected);
        assert_eq!(table.slots[65], name);
    }
}

/// comment・文字列・procedure・FontInfo内の偽Encodingを採用しない。
#[test]
fn fake_definitions_do_not_replace_the_top_level_table() {
    let source = header(
        "% /Encoding 256 array dup 65 /BAD put def\n/Note (/Encoding 256 array dup 65 /BAD put def) def\n/Procedure {/Encoding 256 array dup 65 /BAD put def} bind def\n/FontInfo 1 dict dup begin /Encoding 256 array dup 65 /BAD put def end def\n/Encoding 256 array dup 65 /A put def",
    );
    let table = parse(&DecodeBudget::default(), source.as_bytes()).unwrap();
    assert_eq!(table.slots[65], Some(b"A".as_slice()));
}

/// 後から宣言した未実行procedure内の偽表は、実際の表を上書きしない。
#[test]
fn later_unexecuted_procedure_cannot_replace_encoding() {
    let source = header(
        "/Encoding 256 array dup 65 /A put def /Later {/Encoding 256 array dup 65 /BAD put def} bind def",
    );
    let table = parse(&DecodeBudget::default(), source.as_bytes()).unwrap();
    assert_eq!(table.slots[65], Some(b"A".as_slice()));
}

/// 複数の定義は最後を採用し、eexec以後のbyteはmetadataとして走査しない。
#[test]
fn last_definition_and_encrypted_boundary_are_respected() {
    let source =
        header("/Encoding 256 array dup 65 /A put def\n/Encoding 256 array dup 65 /B put def")
            + "/Encoding 256 array dup 65 /BAD put def";
    let table = parse(&DecodeBudget::default(), source.as_bytes()).unwrap();
    assert_eq!(table.slots[65], Some(b"B".as_slice()));
}

/// 動的な式・範囲外・不完全な表を、部分的に分かったEncodingとして成功させない。
#[test]
fn unsupported_expressions_and_invalid_slots_are_unverified() {
    for body in [
        "/Encoding MakeEncoding def",
        "/Encoding 256 array dup 256 /A put def",
        "/Encoding 256 array dup -1 /A put def",
        "/Encoding 256 array dup 65 /A def",
        "/Encoding 256 array dup 65 /A put",
        "/Encoding 256 array dup 65 /A put def Encoding 65 /B put",
        "/Encoding 256 array dup 65 /A put def ChangeEncoding",
    ] {
        assert!(
            parse(&DecodeBudget::default(), header(body).as_bytes()).is_none(),
            "{body}"
        );
    }
}

/// 256slot全体は確保前に共通metadataとallocationへ予約する。
#[test]
fn slot_capacity_has_an_inclusive_metadata_boundary() {
    let source = header("/Encoding 256 array dup 65 /A put def");
    let required = (256 * std::mem::size_of::<Option<&[u8]>>()) as u64;
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: required,
        ..Default::default()
    });
    assert!(parse(&budget, source.as_bytes()).is_some());
    assert!(budget.check().is_ok());
    let short = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: required - 1,
        ..Default::default()
    });
    assert!(parse(&short, source.as_bytes()).is_none());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual }) if limit == required - 1 && actual == required)
    );
}

/// 開始前のキャンセルは、未知表という通常結果にせず停止理由を保持する。
#[test]
fn cancellation_stops_before_table_allocation() {
    let token = crate::ParseCancellation::new();
    token.cancel();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(parse(&budget, header("/Encoding StandardEncoding def").as_bytes()).is_none());
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
}

/// TeXが使用する既知のfont cache前処理は、後続の静的Encodingを隠さない。
#[test]
fn font_cache_preamble_does_not_hide_static_encoding() {
    let preamble = "FontDirectory/Owned known{/Owned findfont dup/UniqueID known{dup /UniqueID get 1234567 eq exch/FontType get 1 eq and}{pop false}ifelse {save true}{false}ifelse}{false}ifelse\n";
    let source = preamble.to_owned() + &header("/Encoding 256 array dup 65 /B put def");
    let table = parse(&DecodeBudget::default(), source.as_bytes()).unwrap();
    assert_eq!(table.slots[65], Some(b"B".as_slice()));
}

/// 前処理の条件分岐へ追加したEncoding変更を、既知のcache慣用句として通さない。
#[test]
fn font_cache_preamble_rejects_unrecognized_executed_code() {
    let preamble = "FontDirectory/Owned known{/Owned findfont dup/UniqueID known{dup /UniqueID get 1234567 eq exch/FontType get 1 eq and}{pop false}ifelse {save true}{false /Encoding 256 array dup 65 /C put def}ifelse}{false}ifelse\n";
    let source = preamble.to_owned() + &header("/Encoding 256 array dup 65 /B put def");
    assert!(parse(&DecodeBudget::default(), source.as_bytes()).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// どのbyte位置への代入でも、上書き後の名と隣の未指定slotを保つ。
    #[test]
    fn assignment_is_local_and_last_wins(code in 0u8..=255, name in "[A-Za-z]{1,12}") {
        let source = header(&format!("/Encoding 256 array dup {code} /First put dup {code} /{name} put def"));
        let budget = DecodeBudget::default();
        let table = parse(&budget, source.as_bytes()).unwrap();
        prop_assert_eq!(table.slots[usize::from(code)], Some(name.as_bytes()));
        prop_assert_eq!(table.slots[usize::from(code.wrapping_add(1))], None);
        prop_assert!(budget.check().is_ok());
    }
}
