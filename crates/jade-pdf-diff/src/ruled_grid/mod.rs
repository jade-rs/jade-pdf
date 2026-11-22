//! 表の罫線が囲む閉領域を、原文の文字を変更せず調べるための部品。
mod geometry;

#[cfg(not(target_arch = "wasm32"))]
mod sdk_contract_tests;
