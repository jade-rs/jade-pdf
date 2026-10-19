//! ToUnicode の累積処理数、整数境界と停止状態の回帰。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 任意の小さな処理上限を持つ解析予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_tounicode_entries: limit,
        ..Default::default()
    })
}

/// 範囲展開前に累積数を検査し、型付き上限エラーを保持する。
fn assert_limit(budget: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
        resource: "ToUnicode entries", limit: found, actual: observed,
    }) if found == limit && observed == actual));
}

/// scalar 範囲は inclusive 境界まで読み、一件下では展開を開始しない。
#[test]
fn scalar_range_reserves_before_expansion() {
    let source = b"1 beginbfrange\n<0001> <0003> <0041>\nendbfrange";
    let allowed = budget(3);
    assert_eq!(parse_tounicode_cmap(&allowed, source).mappings.len(), 3);
    assert!(allowed.check().is_ok());
    let denied = budget(2);
    assert!(parse_tounicode_cmap(&denied, source).is_empty());
    assert_limit(&denied, 2, 3);
}

/// 無効な Unicode scalar も反復回数へ加算し、空 mapping で予算を回避しない。
#[test]
fn invalid_unicode_scalars_still_consume_work() {
    let state = budget(2);
    parse_tounicode_cmap(
        &state,
        b"1 beginbfrange\n<0001> <0003> <00110000>\nendbfrange",
    );
    assert_limit(&state, 2, 3);
}

/// 別 CMap の再解析と重複 key の上書きも共通の累積処理数へ加算する。
#[test]
fn duplicate_and_reparsed_entries_are_cumulative() {
    let state = budget(3);
    let source = b"2 beginbfchar\n<01> <0041>\n<01> <0042>\nendbfchar";
    assert_eq!(parse_tounicode_cmap(&state, source).mappings.len(), 1);
    assert!(state.check().is_ok());
    parse_tounicode_cmap(&state, source);
    assert!(state.is_stopped());
    assert!(parse_tounicode_cmap(&state, source).is_empty());
    assert_limit(&state, 3, 4);
}

/// code space の vector 増加も同じ上限で止める。
#[test]
fn codespace_entries_share_the_mapping_budget() {
    let state = budget(1);
    parse_tounicode_cmap(&state, b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<01> <0041>\nendbfchar");
    assert_limit(&state, 1, 2);
}

/// 配列形式は range と列挙 destination の小さい方だけ処理する。
#[test]
fn array_range_counts_only_destinations_within_source_range() {
    for source in [
        b"1 beginbfrange\n<01> <03> [<0041> <0042>]\nendbfrange".as_slice(),
        b"1 beginbfrange\n<01> <02> [<0041> <0042> <0043>]\nendbfrange".as_slice(),
    ] {
        let allowed = budget(2);
        assert_eq!(parse_tounicode_cmap(&allowed, source).mappings.len(), 2);
        assert!(allowed.check().is_ok());
        let denied = budget(1);
        assert!(parse_tounicode_cmap(&denied, source).is_empty());
        assert_limit(&denied, 1, 2);
    }
}

/// u32 最大 source で余剰 destination は加算前に無視し、overflow しない。
#[test]
fn array_at_u32_max_ignores_excess_destinations() {
    let state = budget(1);
    let cmap = parse_tounicode_cmap(
        &state,
        b"1 beginbfrange\n<FFFFFFFF> <FFFFFFFF> [<0041> <0042>]\nendbfrange",
    );
    assert_eq!(cmap.mappings.get("FFFFFFFF").unwrap(), "A");
    assert!(state.check().is_ok());
}

/// destination の加算 overflow は panic/wrap せず明示的な不正 CMap エラーを返す。
#[test]
fn scalar_destination_overflow_is_explicit() {
    let state = budget(2);
    parse_tounicode_cmap(&state, b"1 beginbfrange\n<01> <02> <FFFFFFFF>\nendbfrange");
    let error = state.check().expect_err("overflow must be rejected");
    assert!(
        error.to_string().contains("invalid ToUnicode CMap"),
        "{error}"
    );
}

/// 逆転範囲は処理しないためゼロ予算でも追加 allocation しない。
#[test]
fn reversed_range_is_empty() {
    let state = budget(0);
    assert!(
        parse_tounicode_cmap(&state, b"1 beginbfrange\n<02> <01> <0041>\nendbfrange").is_empty()
    );
    assert!(state.check().is_ok());
}

/// 全 u32 範囲の cardinality を切り捨てず、43億回の展開開始前に拒否する。
#[test]
fn full_u32_range_is_rejected_before_iteration() {
    let state = budget(1_000_000);
    assert!(
        parse_tounicode_cmap(
            &state,
            b"1 beginbfrange\n<00000000> <FFFFFFFF> <00000000>\nendbfrange"
        )
        .is_empty()
    );
    assert_limit(&state, 1_000_000, 4_294_967_296);
}

/// bfchar の不正 destination も一回の試行に数える。
#[test]
fn invalid_bfchar_destination_is_charged() {
    let state = budget(0);
    parse_tounicode_cmap(&state, b"1 beginbfchar\n<01> <FFF>\nendbfchar");
    assert_limit(&state, 0, 1);
}

/// 借用 token 走査への整理でも小文字 hex・日本語・code space の lookup を保つ。
#[test]
fn lowercase_hex_and_utf16_array_keep_lookup() {
    let state = budget(4);
    let cmap = parse_tounicode_cmap(&state, b"1 begincodespacerange\n<aa> <ff>\nendcodespacerange\n1 beginbfchar\n<ab> <65e5>\nendbfchar\n1 beginbfrange\n<ac> <ad> [<672c> <8a9e>]\nendbfrange");
    assert_eq!(cmap.mappings.get("AB").unwrap(), "日");
    assert_eq!(cmap.mappings.get("AC").unwrap(), "本");
    assert_eq!(cmap.mappings.get("AD").unwrap(), "語");
    assert_eq!(cmap.code_space_ranges.len(), 1);
    assert!(state.check().is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, failure_persistence: None, ..ProptestConfig::default() })]
    /// 独立した整数 cardinality モデルで scalar/配列形式の予算境界を確認する。
    #[test]
    fn generated_ranges_obey_inclusive_budget(start in 0u32..128, count in 1u32..32, array in any::<bool>()) {
        let end = start + count - 1;
        let destination = if array {
            format!("[{}]", vec!["<0041>"; count as usize].join(" "))
        } else { "<0041>".into() };
        let source = format!("1 beginbfrange\n<{start:04X}> <{end:04X}> {destination}\nendbfrange");
        let allowed = budget(u64::from(count));
        prop_assert_eq!(parse_tounicode_cmap(&allowed, source.as_bytes()).mappings.len(), count as usize);
        prop_assert!(allowed.check().is_ok());
        let denied = budget(u64::from(count - 1));
        prop_assert!(parse_tounicode_cmap(&denied, source.as_bytes()).is_empty());
        assert_limit(&denied, u64::from(count - 1), u64::from(count));
    }
}

/// mapping payload のみを小さい上限で制限する予算を作る。
fn byte_budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_tounicode_bytes: limit,
        ..Default::default()
    })
}

/// payload 超過は byte 資源名と最初の超過量を保持する。
fn assert_byte_limit(state: &DecodeBudget, limit: u64, actual: u64) {
    assert!(matches!(state.check(), Err(ParseError::ResourceLimit {
        resource: "ToUnicode mapping bytes", limit: found, actual: observed,
    }) if found == limit && observed == actual));
}

/// 先行ゼロの長い source 幅を各 key に複製する前に累積 byte を検査する。
#[test]
fn scalar_payload_stops_long_key_amplification() {
    let first = format!("{:064X}", 1);
    let last = format!("{:064X}", 3);
    let source = format!("1 beginbfrange\n<{first}> <{last}> <0041>\nendbfrange");
    let state = byte_budget(130);
    let cmap = parse_tounicode_cmap(&state, source.as_bytes());
    assert!(cmap.is_empty());
    assert!(state.is_stopped());
    assert!(parse_tounicode_cmap(&state, source.as_bytes()).is_empty());
    assert_byte_limit(&state, 130, 195);
}

/// bfchar は odd key の padding と BOM 除去後の正確な UTF-8 byte 数を数える。
#[test]
fn bfchar_payload_counts_padding_bom_and_surrogates() {
    let source = b"1 beginbfchar\n<abc> <FEFF65E5D83DDE000041>\nendbfchar";
    let allowed = byte_budget(12);
    let cmap = parse_tounicode_cmap(&allowed, source);
    assert_eq!(cmap.mappings.get("ABC0").unwrap(), "日😀A");
    assert!(allowed.check().is_ok());
    let denied = byte_budget(11);
    assert!(parse_tounicode_cmap(&denied, source).is_empty());
    assert_byte_limit(&denied, 11, 12);
}

/// 同じ key の上書きと解放済み CMap の再解析でも予約量は返却しない。
#[test]
fn payload_duplicates_and_reparses_are_cumulative() {
    let state = byte_budget(8);
    let source = b"2 beginbfchar\n<01> <0041>\n<01> <0042>\nendbfchar";
    assert_eq!(parse_tounicode_cmap(&state, source).mappings.len(), 1);
    assert!(state.check().is_ok());
    assert!(parse_tounicode_cmap(&state, source).is_empty());
    assert_byte_limit(&state, 8, 9);
}

/// 配列の長い UTF-16 destination は String 化する前に拒否する。
#[test]
fn array_payload_checks_long_destination_before_insertion() {
    let source = format!(
        "1 beginbfrange\n<01> <01> [<{}>]\nendbfrange",
        "65E5".repeat(32)
    );
    let state = byte_budget(97);
    assert!(parse_tounicode_cmap(&state, source.as_bytes()).is_empty());
    assert_byte_limit(&state, 97, 98);
}

/// code space と不正 UTF-16 は mapping String を確保せず、ゼロ byte 予算で処理する。
#[test]
fn codespace_and_invalid_utf16_do_not_allocate_payload() {
    let state = byte_budget(0);
    let cmap = parse_tounicode_cmap(
        &state,
        b"1 begincodespacerange\n<0> <f>\nendcodespacerange\n1 beginbfchar\n<01> <D800>\nendbfchar",
    );
    assert!(cmap.is_empty());
    assert_eq!(cmap.code_space_ranges.len(), 1);
    assert_eq!(cmap.code_space_ranges[0].end, 0xf0);
    assert!(state.check().is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, failure_persistence: None, ..ProptestConfig::default() })]
    /// ASCII destination の独立 byte モデルで scalar/array/bfchar の inclusive 境界を確認する。
    #[test]
    fn generated_payload_obeys_exact_byte_budget(width in 4usize..48, count in 1usize..8, variant in 0u8..3) {
        let source = match variant {
            0 => format!("1 beginbfrange\n<{:0width$X}> <{:0width$X}> <0041>\nendbfrange", 1, count),
            1 => format!("1 beginbfrange\n<{:0width$X}> <{:0width$X}> [{}]\nendbfrange", 1, count, vec!["<0041>"; count].join(" ")),
            _ => format!("{count} beginbfchar\n{}\nendbfchar", (1..=count).map(|code| format!("<{code:0width$X}> <0041>")).collect::<Vec<_>>().join("\n")),
        };
        let normalized_width = width + usize::from(variant == 2 && !width.is_multiple_of(2));
        let total = (count * (normalized_width + 1)) as u64;
        let allowed = byte_budget(total);
        prop_assert_eq!(parse_tounicode_cmap(&allowed, source.as_bytes()).mappings.len(), count);
        prop_assert!(allowed.check().is_ok());
        let denied = byte_budget(total - 1);
        prop_assert!(parse_tounicode_cmap(&denied, source.as_bytes()).is_empty());
        assert_byte_limit(&denied, total - 1, total);
    }

    /// 不正 surrogate と末尾不足を含め、配列なし UTF-16 復号が従来の結果と一致する。
    #[test]
    fn borrowed_utf16_matches_legacy_decoder(units in prop::collection::vec(any::<u16>(), 1..32), tail in 0u8..4) {
        let mut hex = units.iter().map(|unit| format!("{unit:04X}")).collect::<String>();
        hex.extend(std::iter::repeat_n('F', usize::from(tail)));
        let source = format!("1 beginbfchar\n<01> <{hex}>\nendbfchar");
        let state = byte_budget(1024);
        let cmap = parse_tounicode_cmap(&state, source.as_bytes());
        let expected = decode_hex_utf16be(&hex);
        prop_assert_eq!(cmap.mappings.get("01"), expected.as_ref());
        prop_assert!(state.check().is_ok());
    }
}
