//! 部分clipの文字所属を保存し、短いobjectごとに全ページindexを再確保しない。
use super::{budget, owned_text_pdf, with_native_bytes};

/// 同じ字面でも位置が異なる160個のowned text objectを作る。
fn distinct_partial_objects() -> String {
    (0..160)
        .map(|i| {
            let x = 5 + (i % 20) * 9;
            let y = 10 + (i / 20) * 20;
            format!(
                "q {} {} 1 14 re W n BT /F 7 Tf {x} {y} Td (W) Tj ET Q ",
                x + 3,
                y - 2,
            )
        })
        .collect()
}

/// 共有indexなら256KiBで扱えるページを、object数×全文字数の予約で拒否しない。
#[test]
fn distinct_partial_text_objects_use_shared_character_index_budget() {
    let source = owned_text_pdf(&distinct_partial_objects());
    with_native_bytes(&source, |page| {
        let text = page.text().unwrap();
        assert!(text.len() >= 160);
        let expected = crate::pdfium_clip::excluded(&budget(10_000_000), page.objects(), &text)
            .expect("large-budget native ownership reference");
        assert!(!expected.is_empty());
        let actual = crate::pdfium_clip::excluded(&budget(256 * 1024), page.objects(), &text)
            .expect("distinct partial objects must share page character index workspace");
        assert_eq!(actual, expected);
    });
}
