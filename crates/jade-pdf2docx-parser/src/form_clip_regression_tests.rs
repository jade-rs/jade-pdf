//! 訪問stackの予算だけではClip頂点配列を制限できない回帰。
use super::*;
use crate::ParseError;

/// ページ/Form本文保存とroot Frameまでの容量を独立に計算する。
fn baseline(bytes: &[u8]) -> u64 {
    storage_regression_tests::preparation(bytes)
        + (std::mem::size_of::<usize>()
            + std::mem::size_of::<(usize, Rc<Vec<u8>>)>()
            + 6
            + 2 * (std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>())
            + 4 * std::mem::size_of::<Option<(u32, Option<Rc<Vec<u8>>>)>>()
            + 4 * std::mem::size_of::<Frame<'_>>()) as u64
}

/// 通常のForm描画が成功してから、Clip容量不足時の通知停止を検査する。
fn assert_rejected(extra: u64) {
    let bytes = storage_regression_tests::fixture();
    let unrestricted = storage_regression_tests::budget(u64::MAX);
    let mut complete = 0;
    visit(
        &unrestricted,
        &bytes,
        &mut QualityReport::default(),
        |_, _, _, _, _, _| complete += 1,
    );
    unrestricted.check().unwrap();
    assert_eq!(complete, 1);
    let target = storage_regression_tests::budget(baseline(&bytes) + extra);
    let mut visits = 0;
    visit(
        &target,
        &bytes,
        &mut QualityReport::default(),
        |_, _, _, _, _, _| visits += 1,
    );
    assert_eq!(visits, 0, "Clipの保存に成功する前に描画を通知しない");
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 訪問slotまでの容量があっても、ページ矩形の頂点は予算外に確保しない。
#[test]
fn page_clip_requires_capacity() {
    assert_rejected(0);
}

/// ページ矩形までの容量があっても、子の頂点と交差配列は別途予約する。
#[test]
fn child_clip_requires_capacity() {
    assert_rejected((4 * std::mem::size_of::<[f64; 2]>()) as u64);
}
