#![cfg(feature = "onnx")]
use jade_pdf2docx_ocr::{OcrPageContent, OcrRequest, PageOcrSession};
use std::path::PathBuf;

mod common;
use common::edit_distance;

/// CERの分子は全角記号・空白・数値・末尾の誤認識もそのまま含める。
#[test]
fn character_error_metric_counts_every_character() {
    assert_eq!(edit_distance("契約。", "契約00"), 2);
    assert_eq!(edit_distance("万円、支払", "万円、 支払"), 1);
    assert_eq!(edit_distance("住所：", "住所:"), 1);
    assert_eq!(edit_distance("", "追記"), 2);
    assert_eq!(edit_distance("100万円", "100万円"), 0);
}
