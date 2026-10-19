//! Form訪問stack・q/Q保存とBT位置対応の保存容量を迂回する回帰。
use super::*;
use crate::{ParseError, ParseLimits};

/// metadata容量だけを制限する予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 容量超過の型付きエラーを検査する。
fn assert_limit(target: &DecodeBudget) {
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// BT対応だけでも新しい保存slotには容量予約が必要。
#[test]
fn text_matrix_slots_require_capacity() {
    let target = budget(0);
    let matrices = text_matrices(&target, b"BT ET", IDENTITY);
    assert!(matrices.is_empty());
    assert_limit(&target);
}

/// BT以前のq退避にも容量予約が必要。
#[test]
fn text_matrix_graphics_stack_requires_capacity() {
    let target = budget(0);
    let matrices = text_matrices(&target, b"q", IDENTITY);
    assert!(matrices.is_empty());
    assert_limit(&target);
}

/// 停止のエラーを消費した後もBT対応の部分結果を返さない。
#[test]
fn text_matrix_scan_does_not_resume_after_error_consumption() {
    let target = budget(0);
    assert!(target.metadata_vector::<u8>(1).is_none());
    assert_limit(&target);
    assert!(text_matrices(&target, b"BT ET", IDENTITY).is_empty());
}

/// ページ本文・共有管理領域・ページClipだけの容量を独立に計算する。
fn page_storage_capacity(content_length: usize) -> u64 {
    (std::mem::size_of::<usize>()
        + std::mem::size_of::<(usize, Rc<Vec<u8>>)>()
        + content_length
        + 1
        + std::mem::size_of::<Vec<u8>>()
        + 2 * std::mem::size_of::<usize>()
        + 4 * std::mem::size_of::<[f64; 2]>()) as u64
}

/// 本文連結とpayload cache、一回の子Clipに必要なmetadata容量を独立に計算する。
fn storage_capacity(content_length: usize) -> u64 {
    page_storage_capacity(content_length)
        + (std::mem::size_of::<Vec<u8>>()
            + 2 * std::mem::size_of::<usize>()
            + 4 * std::mem::size_of::<Option<(u32, Option<Rc<Vec<u8>>>)>>()
            + 24 * std::mem::size_of::<[f64; 2]>()) as u64
}

/// 本文保存までは可能な予算で、訪問stackを予算外に確保しない。
#[test]
fn visitor_root_frame_requires_capacity() {
    let bytes = storage_regression_tests::fixture();
    let target = storage_regression_tests::budget(
        storage_regression_tests::preparation(&bytes) + page_storage_capacity(5),
    );
    let mut visits = 0;
    visit(
        &target,
        &bytes,
        &mut QualityReport::default(),
        |_, _, _, _, _, _| visits += 1,
    );
    assert_eq!(visits, 0);
    assert_limit(&target);
}

/// rootの訪問slot容量が残っていても、q/Q保存列には別の容量が必要。
#[test]
fn visitor_graphics_stack_requires_capacity() {
    let bytes = String::from_utf8(storage_regression_tests::fixture())
        .unwrap()
        .replace("/Length 5", "/Length 7")
        .replace("/F Do", "q /F Do")
        .into_bytes();
    let target = storage_regression_tests::budget(
        storage_regression_tests::preparation(&bytes)
            + page_storage_capacity(7)
            + (4 * std::mem::size_of::<Frame<'_>>()) as u64,
    );
    let mut visits = 0;
    visit(
        &target,
        &bytes,
        &mut QualityReport::default(),
        |_, _, _, _, _, _| visits += 1,
    );
    assert_eq!(visits, 0);
    assert_limit(&target);
}

/// 訪問回数を取得して、容量超過時の通知順も検査する。
fn visits(target: &DecodeBudget, bytes: &[u8]) -> usize {
    let mut count = 0;
    visit(
        target,
        bytes,
        &mut QualityReport::default(),
        |_, _, _, _, _, _| count += 1,
    );
    count
}

/// 四段のForm入れ子を繰り返し呼び、訪問slotの再利用を検査する。
fn nested_fixture() -> Vec<u8> {
    let content = "/F3 Do /F3 Do";
    let mut input = format!(
        "%PDF-1.7\n1 0 obj << /Type /Page /Contents 2 0 R /Resources << /XObject << /F3 3 0 R /F4 4 0 R /F5 5 0 R /F6 6 0 R >> >> >> endobj\n2 0 obj << /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        content.len()
    );
    for number in 3..=6 {
        let body = if number < 6 {
            format!("/F{} Do", number + 1)
        } else {
            String::new()
        };
        input.push_str(&format!("{number} 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length {} >>\nstream\n{body}\nendstream\nendobj\n", body.len()));
    }
    input.push_str("%%EOF");
    input.into_bytes()
}

/// 拡張できない子は通知せず、確保済み訪問slotとpayloadは次の呼出しで再利用する。
#[test]
fn nested_frame_growth_precedes_callback_and_reuses_capacity() {
    let bytes = nested_fixture();
    let unrestricted = storage_regression_tests::budget(u64::MAX);
    assert_eq!(visits(&unrestricted, &bytes), 8);
    unrestricted.check().unwrap();
    let control = std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>();
    let cache_slot = std::mem::size_of::<Option<(u32, Option<Rc<Vec<u8>>>)>>();
    let storage = std::mem::size_of::<usize>()
        + std::mem::size_of::<(usize, Rc<Vec<u8>>)>()
        + "/F3 Do /F3 Do".len()
        + 1
        + 5 * control
        + 12 * cache_slot;
    let base = storage_regression_tests::preparation(&bytes)
        + storage as u64
        + (4 * std::mem::size_of::<[f64; 2]>()) as u64;
    let child_clip = (24 * std::mem::size_of::<[f64; 2]>()) as u64;
    let frame_slot = std::mem::size_of::<Frame<'_>>() as u64;
    let target = storage_regression_tests::budget(base + 4 * child_clip + 4 * frame_slot);
    assert_eq!(visits(&target, &bytes), 3);
    assert_limit(&target);
    let target = storage_regression_tests::budget(base + 8 * child_clip + 12 * frame_slot);
    assert_eq!(visits(&target, &bytes), 8);
    target.check().unwrap();
}

/// q/Q退避は最大同時深度の容量を再利用し、容量境界を含む。
#[test]
fn visitor_graphics_capacity_is_reused_and_inclusive() {
    let content = "q Q q Q q q q q Q Q Q Q /F Do";
    let bytes = String::from_utf8(storage_regression_tests::fixture())
        .unwrap()
        .replace("/Length 5", &format!("/Length {}", content.len()))
        .replace("/F Do", content)
        .into_bytes();
    let unrestricted = storage_regression_tests::budget(u64::MAX);
    assert_eq!(visits(&unrestricted, &bytes), 1);
    unrestricted.check().unwrap();
    let capacity = storage_regression_tests::preparation(&bytes)
        + storage_capacity(content.len())
        + (4 * std::mem::size_of::<Frame<'_>>() + 4 * std::mem::size_of::<Saved<'_>>()) as u64;
    let target = storage_regression_tests::budget(capacity - 1);
    assert_eq!(visits(&target, &bytes), 0);
    assert_limit(&target);
    let target = storage_regression_tests::budget(capacity);
    assert_eq!(visits(&target, &bytes), 1);
    target.check().unwrap();
}

/// Formへ継承するFont名と色はコピー前に累積予約し、境界値を含む。
#[test]
fn inherited_text_payload_capacity_is_inclusive_and_stop_is_retained() {
    let alias = "SelectedFont";
    let content = format!("BT /{alias} 20 Tf ET 1 0 0 rg /F Do");
    let bytes = String::from_utf8(storage_regression_tests::fixture())
        .unwrap()
        .replace("/Length 5", &format!("/Length {}", content.len()))
        .replace("/F Do", &content)
        .into_bytes();
    let capacity = storage_regression_tests::preparation(&bytes)
        + storage_capacity(content.len())
        + (4 * std::mem::size_of::<Frame<'_>>() + 2 * (alias.len() + 6)) as u64;
    let target = storage_regression_tests::budget(capacity - 1);
    assert_eq!(visits(&target, &bytes), 0);
    assert_limit(&target);
    assert_eq!(visits(&target, &bytes), 0);
    let target = storage_regression_tests::budget(capacity);
    let mut report = QualityReport::default();
    let mut count = 0;
    visit(&target, &bytes, &mut report, |_, _, _, _, _, inherited| {
        count += 1;
        assert_eq!(inherited.state.font.as_deref(), Some(alias));
        assert_eq!(inherited.state.color.as_deref(), Some("FF0000"));
        assert_eq!(inherited.state.size, 20.);
    });
    assert_eq!(count, 1);
    target.check().unwrap();
}
