use std::fs;

use jade_pdf2docx_parser::parse_pdf;

mod support;
use support::paths::temp_file_path;
use support::pdf_builders::minimal_pdf_with_literal_text;

#[test]
fn parse_pdf_decodes_octal_escapes_in_literal_text() {
    let temp_file = temp_file_path("octal_escape_preview_test");
    // \050='(', \051=')', \134='\\'
    fs::write(
        &temp_file,
        minimal_pdf_with_literal_text("A\\050B\\051C\\134D"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("A(B)C\\D"),
        "octal escapes in literal strings must be decoded"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_nested_parentheses_in_literal_text() {
    let temp_file = temp_file_path("nested_parentheses_literal_test");
    fs::write(&temp_file, minimal_pdf_with_literal_text("A(B(C)D)E"))
        .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("A(B(C)D)E"),
        "balanced nested parentheses must be preserved in literal text"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_backspace_and_formfeed_escapes() {
    let temp_file = temp_file_path("control_escape_preview_test");
    fs::write(&temp_file, minimal_pdf_with_literal_text("A\\bB\\fC"))
        .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("A\u{0008}B\u{000C}C"),
        "backspace/formfeed escapes must be decoded"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_line_continuation_after_backslash() {
    let temp_file = temp_file_path("line_continuation_preview_test");
    fs::write(&temp_file, minimal_pdf_with_literal_text("Hello\\\nWorld"))
        .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("HelloWorld"),
        "backslash-newline should be treated as line continuation"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_mixed_line_continuations() {
    let temp_file = temp_file_path("mixed_line_continuation_preview_test");
    // `\` + 改行（CRLF/LF/CR）が連続しても文字列が連結されることを確認する。
    fs::write(
        &temp_file,
        minimal_pdf_with_literal_text("A\\\r\nB\\\nC\\\rD"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("ABCD"),
        "mixed line continuation escapes must be collapsed"
    );

    let _ = fs::remove_file(temp_file);
}
