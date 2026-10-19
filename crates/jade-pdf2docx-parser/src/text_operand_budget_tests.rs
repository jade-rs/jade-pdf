//! 文字描画operandの一時配列・復号結果を確保前の累積容量で制限する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// metadata保存だけに指定容量を適用する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// Font選択なしで単一operandを復号する。
fn decode(target: &DecodeBudget, raw: &[u8], cmap: &ToUnicodeCMap) -> String {
    let empty = BTreeMap::new();
    decode_text_show_operand(target, raw, cmap, &empty, &BTreeMap::new(), None)
}

/// 単一codeの完成mapをテストの予算外で組み立てる。
fn mapping(value: &str) -> ToUnicodeCMap {
    let mut data = ToUnicodeCMapData::default();
    data.reserve_mapping(&DecodeBudget::default()).unwrap();
    data.insert_mapping("41".into(), value.into());
    data.finish(&DecodeBudget::default())
}

/// raw byteとUTF-8結果の全容量をinclusiveに予約する。
#[test]
fn operand_storage_is_exact_and_inclusive() {
    for (raw, expected, cost) in [
        (b"(abc)".as_slice(), "abc", 6),
        (b"(\\200)".as_slice(), "\u{fffd}", 4),
        (b"<82A0>".as_slice(), "あ", 5),
        (b"<FEFFD83DDE00>".as_slice(), "😀", 10),
    ] {
        let enough = budget(cost);
        assert_eq!(decode(&enough, raw, &ToUnicodeCMap::default()), expected);
        assert!(enough.check().is_ok());
        let short = budget(cost - 1);
        assert_eq!(decode(&short, raw, &ToUnicodeCMap::default()), "");
        assert!(
            matches!(short.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == cost)
        );
        assert!(short.is_stopped());
    }
}

/// CMapの多文字出力も確保前に容量不足を返す。
#[test]
fn mapped_output_and_split_widths_are_bounded() {
    let cmap = mapping("文語");
    for (raw, cost) in [
        (b"(A)".as_slice(), 9),
        (
            b"(AA)".as_slice(),
            6 + std::mem::size_of::<usize>() as u64 + 12,
        ),
    ] {
        let enough = budget(cost);
        assert_eq!(
            decode(&enough, raw, &cmap),
            if raw == b"(A)" {
                "文語"
            } else {
                "文語文語"
            }
        );
        assert!(enough.check().is_ok());
        let short = budget(cost - 1);
        assert!(decode(&short, raw, &cmap).is_empty());
        assert!(
            matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost)
        );
    }
}

/// 解放後も累積し、最初のエラーを取得した後も再開しない。
#[test]
fn consumed_failure_never_resumes_operand_decoding() {
    let target = budget(11);
    assert_eq!(decode(&target, b"(abc)", &ToUnicodeCMap::default()), "abc");
    assert!(decode(&target, b"(abc)", &ToUnicodeCMap::default()).is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit { actual: 12, .. })
    ));
    assert!(decode(&target, b"(x)", &ToUnicodeCMap::default()).is_empty());
    assert!(target.is_stopped());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// ASCIIの再処理回数を含めた累積容量をモデルと比較する。
    #[test]
    fn ascii_operand_capacity_matches_model(length in 1usize..96, repeats in 1usize..6, short in any::<bool>()) {
        let raw = format!("({})", "x".repeat(length));
        let total = (length * 2 * repeats) as u64;
        let target = budget(total - u64::from(short));
        for index in 0..repeats {
            let decoded = decode(&target, raw.as_bytes(), &ToUnicodeCMap::default());
            if short && index + 1 == repeats { prop_assert!(decoded.is_empty()); }
            else { prop_assert_eq!(decoded.len(), length); }
        }
        prop_assert_eq!(target.is_stopped(), short);
        prop_assert_eq!(matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total), short);
    }
}

/// 既存のliteral/hex復号結果と外部encodingのfallbackを維持する。
#[test]
fn bounded_decoder_preserves_encoding_fallbacks() {
    let empty = ToUnicodeCMap::default();
    for (raw, encoding) in [
        (b"<D800>".as_slice(), Some("UniGB-UTF16-H")),
        (b"<00410042>".as_slice(), Some("UniCNS-UCS2-H")),
        (b"<A4A4>".as_slice(), Some("ETen-B5-H")),
        (b"<B0A1>".as_slice(), Some("KSCms-UHC-H")),
        (b"<D6D0>".as_slice(), Some("GBK-EUC-H")),
        (b"<82A0>".as_slice(), Some("90ms-RKSJ-H")),
        (b"<FEFFD800>".as_slice(), None),
        (b"<FEFFD800>".as_slice(), Some("GBK-EUC-H")),
        (b"<EFBBBFE38182>".as_slice(), Some("ETen-B5-H")),
        (b"<41 4>".as_slice(), None),
    ] {
        let expected = parse_pdf_hex_string_with_font_encoding(raw, 0, &empty, encoding)
            .unwrap()
            .0;
        let encodings: BTreeMap<String, String> = encoding
            .into_iter()
            .map(|encoding| ("F".into(), encoding.into()))
            .collect();
        assert_eq!(
            decode_text_show_operand(
                &DecodeBudget::default(),
                raw,
                &empty,
                &encodings,
                &BTreeMap::new(),
                Some("F")
            ),
            expected,
            "{raw:?} / {encoding:?}"
        );
    }
}

/// 固定encoding bufferをまたぐ出力を切り捨てず、正確な要求容量を数える。
#[test]
fn encoding_chunks_are_complete_and_bounded() {
    let raw = format!("<{}>", "82A0".repeat(4097));
    let cost = 4097 * 5;
    let target = budget(cost);
    assert_eq!(
        decode(&target, raw.as_bytes(), &ToUnicodeCMap::default()),
        "あ".repeat(4097)
    );
    assert!(target.check().is_ok());
    let short = budget(cost - 1);
    assert!(decode(&short, raw.as_bytes(), &ToUnicodeCMap::default()).is_empty());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost)
    );
}

/// 対応途中の不一致では部分CMap出力を保存せずliteral UTF-8へfallbackする。
#[test]
fn incomplete_mapping_discards_partial_decode() {
    let target = DecodeBudget::default();
    assert_eq!(decode(&target, b"(AB)", &mapping("文")), "AB");
    assert!(target.check().is_ok());
}

/// literalのCR/CRLFは、UTF-8化前に一つのLF codeへ正規化する。
#[test]
fn literal_line_endings_use_normalized_code_capacity() {
    let target = budget(10);
    assert_eq!(
        decode(&target, b"(a\r\nb\rc)", &ToUnicodeCMap::default()),
        "a\nb\nc"
    );
    assert!(target.check().is_ok());
}
