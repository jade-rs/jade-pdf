//! 空のUnicode宛先と不完全UTF16を、実際のoperand復号経路で検査する。
use super::*;
use proptest::prelude::*;

/// 一つのcodeが空文字を指しても、encoded byteやglyph名へ戻さない。
#[test]
fn empty_bfchar_destinations_do_not_fallback_to_source_bytes() {
    let budget = DecodeBudget::default();
    let cmap = parse_tounicode_body(
        &budget,
        b"3 beginbfchar\n<41> <0041>\n<42> <>\n<43> <0043>\nendbfchar",
    );
    assert_eq!(
        operand::decode_bytes(&budget, b"ABC", true, &cmap, None).as_deref(),
        Some("AC")
    );
    assert_eq!(
        operand::decode_bytes(&budget, b"B", true, &cmap, None).as_deref(),
        Some("")
    );
}

/// 範囲内の空宛先を飛ばして後続codeをずらさず、二byte codeも同じ意味で読む。
#[test]
fn empty_bfrange_array_targets_keep_later_codes_aligned() {
    let budget = DecodeBudget::default();
    let cmap = parse_tounicode_body(
        &budget,
        b"1 beginbfrange\n<0101> <0103> [<0058> <> <005A>]\nendbfrange",
    );
    assert_eq!(
        operand::decode_bytes(&budget, &[1, 1, 1, 2, 1, 3], false, &cmap, None).as_deref(),
        Some("XZ")
    );
    assert_eq!(cmap.mappings.get("0102").map(String::as_str), Some(""));
    assert_eq!(cmap.mappings.get("0103").map(String::as_str), Some("Z"));
}

/// source codeの欠落やUTF16の不足を、有効なprefixの文字として部分採用しない。
#[test]
fn malformed_utf16_and_empty_source_codes_are_not_partial_mappings() {
    let budget = DecodeBudget::default();
    let cmap = parse_tounicode_body(
        &budget,
        b"4 beginbfchar\n<> <0041>\n<41> <004100>\n<42> <0041D800>\n<43> <0043>\nendbfchar",
    );
    assert!(cmap.mappings.get("").is_none());
    assert!(cmap.mappings.get("41").is_none());
    assert!(cmap.mappings.get("42").is_none());
    assert_eq!(cmap.mappings.get("43").map(String::as_str), Some("C"));
    assert!(budget.check().is_ok());
}

/// 奇数桁hexを末尾ゼロでbyte化してから、UTF16全体の完全性を検証する。
#[test]
fn odd_hex_destinations_are_padded_before_utf16_validation() {
    let budget = DecodeBudget::default();
    let cmap = parse_tounicode_body(
        &budget,
        b"3 beginbfchar\n<41> <004>\n<42> <D83DDE0>\n<43> <0041FFF>\nendbfchar",
    );
    assert_eq!(
        operand::decode_bytes(&budget, b"ABC", true, &cmap, None).as_deref(),
        Some("@😀A\u{fff0}")
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意の空/Unicode宛先列を途中で詰めず、非BMPを含む原文どおりに復号する。
    #[test]
    fn empty_targets_preserve_arbitrary_sequence_positions(
        values in prop::collection::vec(prop::option::of(any::<char>().prop_filter("BOM is not text",|c|*c!='\u{FEFF}')),1..32)
    ) {
        let targets=values.iter().map(|value|{
            let hex=value.map(|scalar|{
                let mut units=[0u16;2];
                scalar.encode_utf16(&mut units).iter().map(|u|format!("{u:04X}")).collect::<String>()
            }).unwrap_or_default();
            format!("<{hex}>")
        }).collect::<Vec<_>>().join(" ");
        let source=format!("1 beginbfrange\n<01> <{:02X}> [{targets}]\nendbfrange",values.len());
        let budget=DecodeBudget::default();
        let cmap=parse_tounicode_body(&budget,source.as_bytes());
        let codes=(1..=values.len()).map(|n|n as u8).collect::<Vec<_>>();
        let expected=values.into_iter().flatten().collect::<String>();
        let actual=operand::decode_bytes(&budget,&codes,true,&cmap,None);
        prop_assert_eq!(actual,Some(expected));
        prop_assert!(budget.check().is_ok());
    }

    /// 任意の完全なprefixに続くゼロ終端文字は、最終nibbleの省略で変化しない。
    #[test]
    fn odd_hex_padding_preserves_complete_unicode_sequences(
        prefix in prop::collection::vec(any::<char>().prop_filter("BOM is not text", |c| *c != '\u{feff}'), 0..16),
        last in 0u16..0xd80,
    ) {
        let mut expected = prefix.into_iter().collect::<String>();
        expected.push(char::from_u32(u32::from(last) * 16).unwrap());
        let mut hex = expected.encode_utf16().map(|unit| format!("{unit:04X}")).collect::<String>();
        prop_assert_eq!(hex.pop(), Some('0'));
        let source = format!("1 beginbfchar\n<01> <{hex}>\nendbfchar");
        let budget = DecodeBudget::default();
        let cmap = parse_tounicode_body(&budget, source.as_bytes());
        prop_assert_eq!(operand::decode_bytes(&budget, &[1], true, &cmap, None), Some(expected));
        prop_assert!(budget.check().is_ok());
    }
}
