//! Font fallback参照の字句境界と無視領域を検査する。
use super::*;

/// Font番号の末尾一致を参照として採用しない。
#[test]
fn reference_object_number_requires_complete_token() {
    let target = DecodeBudget::default();
    let maps = build(
        &target,
        "/Wrong 11 0 R /Good 1 0 R",
        [(1, 0, "A".to_owned())],
    );
    assert_eq!(maps.get("Wrong"), None);
    assert_eq!(maps.get("Good").map(String::as_str), Some("A"));
    target.check().unwrap();
}

/// コメント・literal/hex文字列内の偽参照と、不完全なR keywordを除外する。
#[test]
fn reference_search_ignores_comments_strings_and_keyword_suffixes() {
    let source =
        "% /Comment 1 0 R\n(/Literal 1 0 R) <2f486578203120302052> /Suffix 1 0 Rubbish /Good 1 0 R";

    let target = DecodeBudget::default();
    let maps = build(&target, source, [(1, 0, "A".to_owned())]);
    for name in ["Comment", "Literal", "Hex", "Suffix"] {
        assert_eq!(maps.get(name), None);
    }
    assert_eq!(maps.get("Good").map(String::as_str), Some("A"));
    target.check().unwrap();
}

/// PDFの空白とコメントは参照を分離でき、Rの直後の辞書delimiterも許す。
#[test]
fn reference_search_accepts_pdf_trivia_and_delimiters() {
    let target = DecodeBudget::default();
    let maps = build(
        &target,
        "<< /F\t1 % number\r\n0\x0cR>>",
        [(1, 0, "A".to_owned())],
    );
    assert_eq!(maps.get("F").map(String::as_str), Some("A"));
    target.check().unwrap();
}

/// nameと参照の間に別の値がある場合、前のnameを誤採用しない。
#[test]
fn reference_must_be_immediate_name_value() {
    for source in [
        "/F (note) 1 0 R",
        "/F [1 0 R]",
        "/F 7 1 0 R",
        "/F /#gg 1 0 R",
    ] {
        let target = DecodeBudget::default();
        assert!(build(&target, source, [(1, 0, "A".to_owned())]).is_empty());
        target.check().unwrap();
    }
}

/// stream本文内の偽参照は直接Lengthに従い除外し、本文内endstreamにも惑わされない。
#[test]
fn stream_payload_references_are_not_font_aliases() {
    let payload = "endstream /Fake 1 0 R";
    let source = format!(
        "9 0 obj << /Length {} >>\nstream\n{payload}\nendstream\nendobj\n/Good 1 0 R",
        payload.len()
    );
    let target = DecodeBudget::default();
    let maps = build(&target, &source, [(1, 0, "A".to_owned())]);
    assert!(maps.get("Fake").is_none());
    assert_eq!(maps.get("Good").map(String::as_str), Some("A"));
    target.check().unwrap();
}

/// 表現が似た不正なnumber/generation/keywordを完全参照へ丸めない。
#[test]
fn malformed_reference_tokens_are_never_normalized_into_aliases() {
    for source in [
        "/F 1.0 0 R",
        "/F -1 0 R",
        "/F 4294967296 0 R",
        "/F 1 65536 R",
        "/F 1 -1 R",
        "/F 1 0.0 R",
        "/F 1 0/R",
        "/F 1 0 (R)",
        "/F 1 0 Rubbish",
    ] {
        let target = DecodeBudget::default();
        assert!(build(&target, source, [(1, 0, "A".to_owned())]).is_empty());
        target.check().unwrap();
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// 非0世代と任意のPDF空白/コメントを含む完全参照だけを独立expectationへ一致させる。
    #[test]
    fn valid_reference_model_excludes_decoys(
        number in 1u32..100000,
        generation in 1u16..u16::MAX,
        gap in prop::sample::select(vec![" ", "\t", "\r\n", "\x00", "\x0c", " % ignored\r\n"]),
    ) {
        let source = format!("% /Comment {number} {generation} R\n(/Literal {number} {generation} R) /Prefix 1{number} {generation} R /Stale {number} {} R /Suffix {number} {generation} Rubbish /#e9{gap}{number}{gap}{generation}{gap}R", generation - 1);
        let target = DecodeBudget::default();
        let maps = build(&target, &source, [(number, generation, "A".to_owned())]);
        for name in ["Comment", "Literal", "Prefix", "Stale", "Suffix"] { prop_assert!(maps.get(name).is_none()); }
        prop_assert_eq!(maps.get("é").map(String::as_str), Some("A"));
        target.check().unwrap();
    }

    /// 偽物だけの入力はmetadata容量0でも保存要求を出さない。
    #[test]
    fn decoy_only_inputs_need_no_metadata_storage(number in 1u32..100000, generation in any::<u16>()) {
        let source = format!("% /Comment {number} {generation} R\n(/Literal {number} {generation} R) /Wrong 1{number} {generation} R /Suffix {number} {generation} Rfoo");
        let target = DecodeBudget::new(&crate::ParseLimits { max_total_metadata_workspace_bytes: 0, ..Default::default() });
        prop_assert!(build(&target, &source, [(number, generation, "A".to_owned())]).is_empty());
        target.check().unwrap();
    }
}
