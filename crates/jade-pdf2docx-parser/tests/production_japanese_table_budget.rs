//! 全ページ・権利・独立goldを先に確認した日本語表を、既定の解析予算で扱う。
#![cfg(not(target_arch = "wasm32"))]

use jade_pdf2docx_parser::{ParseLimits, parse_pdf_with_limits};
use std::path::Path;

/// コピー許可のある原本をハッシュで固定し、二ページの表でmetadata予算を使い切らない。
#[test]
#[ignore = "requires reviewed NTA original and pinned native Pdfium; set JADE_JAPANESE_TABLE_HOLDOUT"]
fn reviewed_japanese_two_page_table_fits_default_metadata_workspace() {
    let input = std::env::var("JADE_JAPANESE_TABLE_HOLDOUT")
        .expect("set the independently reviewed original PDF path");
    let bytes = std::fs::read(&input).unwrap();
    assert_eq!(bytes.len(), 604_798);
    let hash = hmac_sha256::Hash::hash(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        hash,
        "a74ef38911860b41868e5f1b7d8f870d2ff870ad2dcfd16c637a4e15abfd1e14"
    );
    let parsed = parse_pdf_with_limits(Path::new(&input), &ParseLimits::default())
        .expect("reviewed two-page Japanese table must fit default metadata workspace");
    assert_eq!(parsed.page_count, 2);
    assert!(!parsed.text_elements.is_empty());
}
