//! 権利・全ページを確認済みの既知国立天文台PDFで、nativeの空白と登録CID対応を検証する。
#![cfg(not(target_arch = "wasm32"))]
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf};
use std::path::PathBuf;

/// 外部対照をhashで固定し、SDKを明示したfile APIだけで解析する。
fn reviewed_document(variable: &str, expected: &str) -> ParsedDocument {
    assert!(
        std::env::var_os("PDFIUM_LIB_PATH").is_some(),
        "pinned SDK is required"
    );
    let path = PathBuf::from(std::env::var_os(variable).expect("reviewed source path is required"));
    let bytes = std::fs::read(&path).unwrap();
    let digest = hmac_sha256::Hash::hash(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(digest, expected);
    parse_pdf(&path).unwrap()
}

/// 登録CIDの解決が成功しても、SDKが識別した月日間の論理的区切りを消さない。
#[test]
#[ignore = "requires reviewed NAOJ excerpt in JADE_NAOJ_CID_EXCERPT and pinned PDFIUM_LIB_PATH"]
fn native_registered_cid_page_preserves_numeric_date_separators() {
    let document = reviewed_document(
        "JADE_NAOJ_CID_EXCERPT",
        "ac3b71b87ba7f08e9094639c2108926ba0f79a9d21fd59e582c8d09925e36078",
    );
    let texts = document
        .text_elements
        .iter()
        .map(|e| e.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        texts.iter().any(|text| text.trim() == "1 12"),
        "adult holiday month/day separator lost: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.trim() == "7 20"),
        "marine holiday month/day separator lost: {texts:?}"
    );
}

/// CID18b5は公式Adobe表で芒＋U+E0100となり、SDKのbase scalarだけで置き換えない。
#[test]
#[ignore = "requires reviewed whole NAOJ calendar in JADE_NAOJ_CID_WHOLE and pinned PDFIUM_LIB_PATH"]
fn native_registered_cid_keeps_source_variation_sequence() {
    let document = reviewed_document(
        "JADE_NAOJ_CID_WHOLE",
        "ee5f0a743c0e7e577fa4115a07eef7fa752cee9d6ce1022e06305a09f33f3d6d",
    );
    let text = document
        .text_elements
        .iter()
        .map(|e| e.text.as_str())
        .collect::<String>();
    assert!(
        text.contains("芒\u{e0100}種"),
        "official source CID18b5 variation sequence lost"
    );
    assert_eq!(text.chars().filter(|c| *c == '\u{e0100}').count(), 1);
}
