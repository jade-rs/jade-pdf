//! 文書previewの重複索引・正規化キー・完成文字列の累積容量を固定する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// previewのmetadata容量だけを制限する。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 単一literalの復号・初回索引slot・キー・出力の全確保容量を独立に計算する。
fn single_cost(bytes: &[u8], key: &str, shown: &str) -> u64 {
    (bytes.len()
        + 4 * std::mem::size_of::<Option<(String, ())>>()
        + key.len()
        + shown.len()
        + if key.chars().count() >= 1024 {
            2 * key.len()
        } else {
            6 * key.len()
        }) as u64
}

/// 完成出力の1 byte不足で部分結果を返さず、inclusive上限では同じ文字を返す。
#[test]
fn preview_result_capacity_is_inclusive() {
    let bytes = b"BT (Hello) Tj ET";
    let cost = single_cost(bytes, "Hello", "Hello");
    let enough = budget(cost);
    assert_eq!(extract_text_preview_from_pdf_bytes(&enough, bytes), "Hello");
    assert!(enough.check().is_ok());
    let slots = (bytes.len() + 10 + 4 * std::mem::size_of::<Option<(String, ())>>()) as u64;
    for stage_cost in [slots, slots + 5, slots + 10] {
        let short = budget(stage_cost - 1);
        assert!(extract_text_preview_from_pdf_bytes(&short, bytes).is_empty());
        assert!(matches!(short.check(), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", actual, ..
        }) if actual == stage_cost));
        assert!(extract_text_preview_from_pdf_bytes(&short, bytes).is_empty());
    }
}

/// 重複と前後空白は借用検索し、余分なキーと出力を確保しない。
#[test]
fn preview_duplicate_lines_do_not_allocate_again() {
    let bytes = b"BT (  Hello  ) Tj (Hello) Tj ( Hello ) Tj ET";
    let target = budget(single_cost(bytes, "Hello", "Hello") + 126 - 30);
    assert_eq!(extract_text_preview_from_pdf_bytes(&target, bytes), "Hello");
    assert!(target.check().is_ok());
}

/// 同じ入力の再作成も累積し、エラー取得後も処理を再開しない。
#[test]
fn preview_result_recreation_is_cumulative() {
    let bytes = b"BT (Hello) Tj ET";
    let cost = single_cost(bytes, "Hello", "Hello");
    let target = budget(2 * cost - 1);
    assert_eq!(extract_text_preview_from_pdf_bytes(&target, bytes), "Hello");
    assert!(extract_text_preview_from_pdf_bytes(&target, bytes).is_empty());
    assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", actual, ..
    }) if actual == 2 * cost));
    assert!(extract_text_preview_from_pdf_bytes(&target, b"").is_empty());
    assert!(target.is_stopped());
}

/// 後続行の結合で不足した場合も先行行を公開しない。
#[test]
fn preview_join_failure_discards_first_line() {
    let bytes = b"BT (Hello) Tj (World) Tj ET";
    let cost =
        (bytes.len() + 4 * std::mem::size_of::<Option<(String, ())>>() + 10 + 5 + 11 + 60) as u64;
    let enough = budget(cost);
    assert_eq!(
        extract_text_preview_from_pdf_bytes(&enough, bytes),
        "Hello\nWorld"
    );
    assert!(enough.check().is_ok());
    let join_cost = cost - 40;
    let short = budget(join_cost - 1);
    assert!(extract_text_preview_from_pdf_bytes(&short, bytes).is_empty());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == join_cost)
    );
}

/// Unicodeの文字数で切り、切断後に完成文字列を複製しない。
#[test]
fn preview_unicode_limit_preserves_utf8_boundary() {
    let key = "あ".repeat(1100);
    let bytes = format!("BT ({key}) Tj ET");
    let shown = "あ".repeat(1024);
    let target = budget(single_cost(bytes.as_bytes(), &key, &shown));
    assert_eq!(
        extract_text_preview_from_pdf_bytes(&target, bytes.as_bytes()),
        shown
    );
    assert!(target.check().is_ok());
    let short = budget(single_cost(bytes.as_bytes(), &key, &shown) - 1);
    assert!(extract_text_preview_from_pdf_bytes(&short, bytes.as_bytes()).is_empty());
    assert!(matches!(
        short.check(),
        Err(ParseError::ResourceLimit { .. })
    ));
}

/// 停止済み入力は候補iterator自体を進めず、空結果でも停止を保持する。
#[test]
fn preview_stopped_budget_does_not_visit_candidates() {
    let target = budget(0);
    assert!(target.metadata_vector::<u8>(1).is_none());
    assert!(target.check().is_err());
    let candidates = std::iter::from_fn(|| -> Option<String> {
        panic!("停止後に候補を復号してはいけない")
    });
    assert!(preview_result::collect(&target, candidates).is_none());
    assert!(target.is_stopped());
}

/// 可読性のない候補を保存せず、空previewにslot・文字列の予算を使わない。
#[test]
fn preview_noise_has_no_result_allocation() {
    let target = budget(0);
    let candidates = [" ", "x", "()", "\u{0001}\u{0002}"].map(str::to_owned);
    assert_eq!(
        preview_result::collect(&target, candidates).as_deref(),
        Some("")
    );
    assert!(target.check().is_ok());
}

/// 改行だけで1,024文字に達する場合も後続行を切断してUTF-8を保持する。
#[test]
fn preview_limit_includes_separator() {
    let candidates = ["あ".repeat(1023), "次の行".to_owned()];
    let target = DecodeBudget::default();
    let output = preview_result::collect(&target, candidates).expect("十分な予算");
    assert_eq!(output, format!("{}\n", "あ".repeat(1023)));
    assert_eq!(output.chars().count(), 1024);
    assert!(target.check().is_ok());
}

/// 固定長の異なる行で、索引と出力の全拡張容量を独立に数える。
fn distinct_line_cost(count: usize) -> u64 {
    let mut slots = 4usize;
    let mut map_slots = slots;
    while count > slots / 2 {
        slots *= 2;
        map_slots += slots;
    }
    let mut output_capacity = 4usize;
    let mut output_cost = output_capacity;
    for index in 2..=count {
        let needed = index * 5 - 1;
        if needed > output_capacity {
            output_capacity = needed.max(output_capacity * 2);
            output_cost += output_capacity;
        }
    }
    (map_slots * std::mem::size_of::<Option<(String, ())>>() + count * 4 + output_cost) as u64
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// 任意の件数で索引/出力の初回・倍増容量を固定し、重複は再確保しない。
    #[test]
    fn preview_result_growth_matches_capacity_model(count in 1usize..=40) {
        let lines: Vec<String> = (0..count).map(|index| format!("x{index:03}")).collect();
        let cost = distinct_line_cost(count);
        let target = budget(cost);
        let actual = preview_result::collect(&target, lines.iter().cloned().chain(lines.iter().cloned()));
        let expected = lines.join("\n");
        prop_assert_eq!(actual.as_deref(), Some(expected.as_str()));
        prop_assert!(target.check().is_ok());
        let short = budget(cost - 1);
        prop_assert!(preview_result::collect(&short, lines.clone()).is_none());
        prop_assert!(matches!(short.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == cost), "全要求容量で停止する");
        prop_assert!(preview_result::collect(&short, lines).is_none());
    }
}

/// 復号済みの一文字も全確保容量を予約し、1 byte不足なら部分結果を返さない。
#[test]
fn decoded_preview_capacity_is_inclusive() {
    let cost = (4 * std::mem::size_of::<Option<(String, ())>>() + 6) as u64;
    for limit in [cost - 1, cost] {
        let target = budget(limit);
        let mut builder = preview_result::Builder::default();
        assert_eq!(builder.push_decoded(&target, "文").is_some(), limit == cost);
        if limit == cost {
            assert_eq!(builder.finish(&target).as_deref(), Some("文"));
            assert!(target.check().is_ok());
        } else {
            assert!(builder.push_decoded(&target, "!").is_none());
            assert!(builder.finish(&target).is_none());
            assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            }) if actual == cost));
        }
    }
}

/// ページ・Formの重複に相当する前後空白付きの借用入力を再確保しない。
#[test]
fn decoded_preview_duplicate_does_not_allocate() {
    let cost = (4 * std::mem::size_of::<Option<(String, ())>>() + 6) as u64;
    let target = budget(cost);
    let mut builder = preview_result::Builder::default();
    for text in [" 文 ", "文", "\t文\n"] {
        assert_eq!(builder.push_decoded(&target, text), Some(true));
    }
    assert_eq!(builder.finish(&target).as_deref(), Some("文"));
    assert!(target.check().is_ok());
}

/// 二行目のUTF-8と改行の確保に失敗した場合は先行行も公開しない。
#[test]
fn decoded_preview_join_failure_discards_first_line() {
    let cost = (4 * std::mem::size_of::<Option<(String, ())>>() + 3 + 3 + 4 + 8) as u64;
    for limit in [cost - 1, cost] {
        let target = budget(limit);
        let mut builder = preview_result::Builder::default();
        assert_eq!(builder.push_decoded(&target, "文"), Some(true));
        assert_eq!(builder.push_decoded(&target, "𝄞").is_some(), limit == cost);
        if limit == cost {
            assert_eq!(builder.finish(&target).as_deref(), Some("文\n𝄞"));
            assert!(target.check().is_ok());
        } else {
            assert!(builder.finish(&target).is_none());
            assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes", actual, ..
            }) if actual == cost));
        }
    }
}

/// 空白・許可しない制御文字を含む本文は保存用の予算を消費しない。
#[test]
fn decoded_preview_noise_has_no_result_allocation() {
    let target = budget(0);
    let mut builder = preview_result::Builder::default();
    for text in ["", " \t\n", "文\u{0001}"] {
        assert_eq!(builder.push_decoded(&target, text), Some(true));
    }
    assert_eq!(builder.finish(&target).as_deref(), Some(""));
    assert!(target.check().is_ok());
}
