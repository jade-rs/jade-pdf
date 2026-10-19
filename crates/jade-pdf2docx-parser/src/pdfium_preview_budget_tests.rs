//! native previewの連結・正規化を共有容量へ予約する回帰。
use super::{append_text_preview, normalize_pdfium_text_preview};
use crate::{ParseError, ParseLimits, decode::DecodeBudget};

/// 指定したmetadata容量で独立した解析予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 連結の新しい全容量を予約し、超過時は元のpreviewを変更しない。
#[test]
fn native_preview_append_is_bounded_and_atomic() {
    let mut text = "既存".to_owned();
    let initial_capacity = text.capacity();
    let addition = "追加の本文";
    let required = (text.len() + 1 + addition.len()).max(initial_capacity * 2) as u64;
    let stopped = budget(required - 1);
    assert!(append_text_preview(&stopped, &mut text, addition).is_none());
    assert_eq!(text, "既存");
    assert!(matches!(stopped.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", actual, limit,
    }) if actual == required && limit == required - 1));
    let allowed = budget(required);
    assert!(append_text_preview(&allowed, &mut text, addition).is_some());
    assert_eq!(text, "既存\n追加の本文");
    assert!(allowed.check().is_ok());
}

/// 容量再利用は再課金せず、消費済み停止状態では空白候補も処理しない。
#[test]
fn native_preview_append_reuses_capacity_and_preserves_stop() {
    let mut text = String::with_capacity(32);
    let target = budget(0);
    assert!(append_text_preview(&target, &mut text, " 一行 ").is_some());
    assert!(append_text_preview(&target, &mut text, "\r\n ").is_some());
    assert_eq!(text, "一行");
    assert!(target.check().is_ok());
    target.charge_metadata_workspace_bytes(1);
    assert!(target.check().is_err());
    assert!(append_text_preview(&target, &mut text, " ").is_none());
    assert_eq!(text, "一行");
}

/// 正規化の索引とUTF-8出力が同じ予算を使い、停止時は部分結果を返さない。
#[test]
fn native_preview_normalization_has_inclusive_shared_limit() {
    let text = " first \r\n日本語\r\nfirst\n🙂\n";
    let expected = "first\n日本語\n🙂";
    let required = (0..4096)
        .find(|&capacity| normalize_pdfium_text_preview(&budget(capacity), text).is_some())
        .unwrap();
    assert!(
        required > 0,
        "native normalization must reserve its output and index"
    );
    let target = budget(required);
    assert_eq!(
        normalize_pdfium_text_preview(&target, text).as_deref(),
        Some(expected)
    );
    assert!(target.check().is_ok());
    let target = budget(required - 1);
    assert!(normalize_pdfium_text_preview(&target, text).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    assert!(normalize_pdfium_text_preview(&target, "").is_none());
}

proptest::proptest! {
    /// 重複・空白・Unicode・混在改行でも初出順の本文を保持する。
    #[test]
    fn native_preview_normalization_preserves_first_lines(
        lines in proptest::collection::vec("[a-z日本語🙂 ]{0,20}", 0..20),
        crlf in proptest::bool::ANY,
    ) {
        let input = lines.join(if crlf { "\r\n" } else { "\n" });
        let mut unique = Vec::new();
        for line in &lines {
            let line = line.trim();
            if !line.is_empty() && !unique.contains(&line) {
                unique.push(line);
            }
        }
        let expected = unique.join("\n");
        let target = budget(64 * 1024);
        proptest::prop_assert_eq!(normalize_pdfium_text_preview(&target, &input), Some(expected));
        proptest::prop_assert!(target.check().is_ok());
    }
}
