use std::fs;

use jade_pdf2docx_parser::parse_pdf;

mod support;
use support::paths::temp_file_path;
use support::pdf_builders::{
    minimal_pdf_with_malformed_hex_then_valid, minimal_pdf_with_malformed_literal_then_valid,
    minimal_pdf_with_malformed_tj_then_valid,
};

#[test]
/// 未完TJ配列の次のtext objectを本文・previewへ復元する。
fn parse_pdf_recovers_after_malformed_tj_array() {
    let temp_file = temp_file_path("malformed_tj_recovery_test");
    // 先頭の TJ 配列は意図的に壊して、後続の正常テキスト抽出が継続されることを確認する。
    fs::write(
        &temp_file,
        minimal_pdf_with_malformed_tj_then_valid("Broken", "RecoveredLine"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("parser should recover from malformed TJ");
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "RecoveredLine");
    assert!(
        parsed.text_preview.contains("RecoveredLine"),
        "parser must continue and extract text after malformed TJ"
    );
    assert!(
        !parsed.text_preview.contains("Broken"),
        "malformed TJ fragments should be ignored as noise"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
/// 未完literalの後続描画を本文・previewへ復元する。
fn parse_pdf_recovers_after_unclosed_literal() {
    let temp_file = temp_file_path("malformed_literal_recovery_test");
    // 先頭 literal を閉じずに壊し、後続の正常 literal が抽出されることを確認する。
    fs::write(
        &temp_file,
        minimal_pdf_with_malformed_literal_then_valid("Broken", "RecoveredLiteral"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("parser should recover from unclosed literal");
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "RecoveredLiteral");
    assert!(
        parsed.text_preview.contains("RecoveredLiteral"),
        "parser must continue and extract text after malformed literal"
    );
    assert!(
        !parsed.text_preview.contains("Broken"),
        "malformed literal fragment should be ignored as noise"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
/// 未完hexの後続描画を本文・previewへ復元する。
fn parse_pdf_recovers_after_unclosed_hex_string() {
    let temp_file = temp_file_path("malformed_hex_recovery_test");
    // 先頭 hex を閉じずに壊し、後続の正常 literal が抽出されることを確認する。
    fs::write(
        &temp_file,
        minimal_pdf_with_malformed_hex_then_valid("48656C6C6F", "RecoveredAfterHex"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("parser should recover from unclosed hex");
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "RecoveredAfterHex");
    assert!(
        parsed.text_preview.contains("RecoveredAfterHex"),
        "parser must continue and extract text after malformed hex"
    );
    assert!(
        !parsed.text_preview.contains("Hello"),
        "malformed hex fragment should be ignored as noise"
    );

    let _ = fs::remove_file(temp_file);
}
