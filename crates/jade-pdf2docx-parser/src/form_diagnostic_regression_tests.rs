//! Form訪問のFont診断集合・出力slotの予算迂回を再現する。
use super::*;
use crate::{ParseError, QualityIssue, UnsupportedFontReason};

/// 未対応Fontの文字描画を一回だけ行うRootなし互換入力を作る。
fn fixture() -> Vec<u8> {
    named_fixture("F", "BT /F 12 Tf (X) Tj ET")
}

/// Fontの辞書名と文字描画本文を指定した互換入力を作る。
fn named_fixture(name: &str, content: &str) -> Vec<u8> {
    format!("%PDF-1.7\n1 0 obj << /Type /Page /Contents 2 0 R /Resources << /Font << /{name} 4 0 R >> >> >> endobj\n2 0 obj << /Length {} >>\nstream\n{content}\nendstream\nendobj\n4 0 obj << /Type /Font /Subtype /Type3 >> endobj\n%%EOF", content.len()).into_bytes()
}

/// ページ本文の保存とページClipとroot Frameのslotだけの容量を独立に計算する。
fn baseline_capacity(bytes: &[u8]) -> u64 {
    content_capacity(bytes, "BT /F 12 Tf (X) Tj ET".len())
}

/// 任意長の本文保存とページClipとroot Frameだけの容量を独立に計算する。
fn content_capacity(bytes: &[u8], content_length: usize) -> u64 {
    storage_regression_tests::preparation(bytes)
        + (std::mem::size_of::<usize>()
            + std::mem::size_of::<(usize, Rc<Vec<u8>>)>()
            + content_length
            + 1
            + std::mem::size_of::<Vec<u8>>()
            + 2 * std::mem::size_of::<usize>()
            + 4 * std::mem::size_of::<[f64; 2]>()
            + 4 * std::mem::size_of::<Frame<'_>>()) as u64
}

/// 通常入力の診断を確認してから、集合・出力容量を不足させる。
fn assert_rejected(extra: u64) {
    let bytes = fixture();
    let target = storage_regression_tests::budget(u64::MAX);
    let mut report = QualityReport::default();
    visit(&target, &bytes, &mut report, |_, _, _, _, _, _| {
        panic!("no Form")
    });
    target.check().unwrap();
    assert_eq!(
        report.issues,
        [QualityIssue::UnsupportedFont {
            page: 0,
            object_number: Some(4),
            reason: UnsupportedFontReason::Type3,
        }]
    );
    let target = storage_regression_tests::budget(baseline_capacity(&bytes) + extra);
    let mut report = QualityReport::default();
    visit(&target, &bytes, &mut report, |_, _, _, _, _, _| {
        panic!("no Form")
    });
    assert!(
        report.issues.is_empty(),
        "診断の保存容量を確保する前に公開しない"
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 本文とFrameの予算だけでは診断集合を予算外に確保しない。
#[test]
fn font_diagnostic_set_requires_capacity() {
    assert_rejected(0);
}

/// 集合の予算が残っていても診断出力slotには別の容量が必要。
#[test]
fn font_diagnostic_output_requires_capacity() {
    assert_rejected((4 * std::mem::size_of::<Option<(usize, Selection)>>()) as u64);
}

/// 長いescape名の状態保存だけを数え、診断の重複除去では名前を複製しない。
#[test]
fn long_font_names_are_compared_without_additional_name_storage() {
    let suffix = "A".repeat(4096);
    let name = format!("F{suffix}#FF");
    let operand = format!("#46{suffix}#ff");
    let content =
        format!("BT /{operand} 12 Tf (X) Tj /Broken#GG 12 Tf (Y) Tj /{operand} 12 Tf (Z) Tj ET");
    let bytes = named_fixture(&name, &content);
    let capacity = content_capacity(&bytes, content.len())
        + (2 * (suffix.len() + 3)) as u64
        + (4 * std::mem::size_of::<Option<(usize, Selection)>>()
            + 4 * std::mem::size_of::<QualityIssue>()) as u64;
    for (capacity, complete) in [(capacity - 1, false), (capacity, true)] {
        let target = storage_regression_tests::budget(capacity);
        let mut report = QualityReport::default();
        visit(&target, &bytes, &mut report, |_, _, _, _, _, _| {
            panic!("no Form")
        });
        assert_eq!(target.check().is_ok(), complete);
        // 容量不足は最後のTf保存で起き、先に完了した診断は保持する。
        let expected = vec![QualityIssue::UnsupportedFont {
            page: 0,
            object_number: Some(4),
            reason: UnsupportedFontReason::Type3,
        }];
        assert_eq!(report.issues, expected);
    }
}

/// 長いescape名のDoを繰り返しても名前保存用heapを必要としない。
#[test]
fn long_form_names_are_compared_without_additional_name_storage() {
    let suffix = "A".repeat(4096);
    let name = format!("F{suffix}#FF");
    let operand = format!("#46{suffix}#ff");
    let content = format!("/{operand} Do /Broken#GG Do /{operand} Do");
    let bytes = String::from_utf8(storage_regression_tests::fixture())
        .unwrap()
        .replace("/F 3 0 R", &format!("/{name} 3 0 R"))
        .replace("/Length 5", &format!("/Length {}", content.len()))
        .replace("/F Do", &content)
        .into_bytes();
    let capacity = content_capacity(&bytes, content.len())
        + (std::mem::size_of::<Vec<u8>>()
            + 2 * std::mem::size_of::<usize>()
            + 4 * std::mem::size_of::<Option<(u32, Option<Rc<Vec<u8>>>)>>()
            + 2 * 24 * std::mem::size_of::<[f64; 2]>()) as u64;
    let target = storage_regression_tests::budget(capacity);
    let mut visits = 0;
    let mut report = QualityReport::default();
    visit(&target, &bytes, &mut report, |_, _, _, _, _, _| visits += 1);
    target.check().unwrap();
    assert_eq!(visits, 2);
    assert!(report.issues.is_empty());
}
