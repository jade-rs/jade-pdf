use std::fs;

use jade_pdf2docx_parser::{parse_pdf, parse_pdf_bytes};

mod support;
use support::paths::temp_file_path;
use support::pdf_builders::{
    minimal_pdf_with_external_cmap_hex_text, minimal_pdf_with_hex_text,
    minimal_pdf_with_literal_text, minimal_pdf_with_ordered_hex_and_literal,
    minimal_pdf_with_tj_array, minimal_pdf_with_tj_array_numeric_tokens,
    minimal_pdf_with_tj_array_utf16_and_numeric_tokens, minimal_pdf_with_tounicode_bfchar_text,
    minimal_pdf_with_tounicode_bfrange_array_text, minimal_pdf_with_tounicode_bfrange_text,
    minimal_pdf_with_tounicode_disallowed_range_bfchar_text,
    minimal_pdf_with_tounicode_disallowed_width_bfchar_text,
    minimal_pdf_with_tounicode_mixed_width_bfchar_text,
    minimal_pdf_with_tounicode_one_byte_bfchar_text,
};

#[test]
fn parse_pdf_extracts_hex_text_in_text_object() {
    let temp_file = temp_file_path("hex_text_preview_test");
    fs::write(&temp_file, minimal_pdf_with_hex_text("48656c6c6f20544444"))
        .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("Hello TDD"),
        "hex encoded text must be decoded in preview"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_shift_jis_hex_text_in_text_object() {
    let temp_file = temp_file_path("shift_jis_hex_text_preview_test");
    let pdf = minimal_pdf_with_hex_text("8fea8d87");

    let parsed = parse_pdf_bytes(&temp_file, pdf.as_bytes()).expect("parse must succeed");
    assert!(
        parsed.text_preview.contains("場合"),
        "Shift_JIS hex text must be decoded; actual: {:?}",
        parsed.text_preview
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_utf16_external_cmap_hex_text() {
    let pdf = minimal_pdf_with_external_cmap_hex_text("UniKS-UCS2-H", "AC00AC01AC02");

    let parsed = parse_pdf_bytes("external-cmap-utf16.pdf", pdf.as_bytes())
        .expect("parse_pdf_bytes must succeed");

    assert!(
        parsed.text_preview.contains("가각갂"),
        "UniKS-UCS2-H must decode UTF-16BE CID text; actual: {:?}",
        parsed.text_preview
    );
    assert_eq!(parsed.text_elements[0].text, "가각갂");
}

#[test]
fn parse_pdf_decodes_legacy_external_cmap_hex_text() {
    let pdf = minimal_pdf_with_external_cmap_hex_text("KSCms-UHC-H", "b0a1b0a2b0a3");

    let parsed = parse_pdf_bytes("external-cmap-uhc.pdf", pdf.as_bytes())
        .expect("parse_pdf_bytes must succeed");

    assert!(
        parsed.text_preview.contains("가각간"),
        "KSCms-UHC-H must decode legacy encoded CID text; actual: {:?}",
        parsed.text_preview
    );
    assert_eq!(parsed.text_elements[0].text, "가각간");
}

#[test]
fn parse_pdf_extracts_utf16be_hex_text_in_text_object() {
    let temp_file = temp_file_path("utf16_hex_text_preview_test");
    // FEFF + "日本" (U+65E5 U+672C)
    let pdf = minimal_pdf_with_hex_text("FEFF65E5672C");

    let parsed = parse_pdf_bytes(&temp_file, pdf.as_bytes()).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("日本"),
        "utf16be hex encoded text must be decoded in preview"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_tounicode_bfchar_hex_text() {
    let temp_file = temp_file_path("tounicode_bfchar_hex_text_preview_test");
    let pdf_bytes = minimal_pdf_with_tounicode_bfchar_text("00010001");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed = parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes())
        .expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("日日"),
        "ToUnicode bfchar must decode CID hex text; actual: {:?}",
        parsed.text_preview
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_tounicode_bfrange_hex_text() {
    let temp_file = temp_file_path("tounicode_bfrange_hex_text_preview_test");
    let pdf_bytes = minimal_pdf_with_tounicode_bfrange_text("00010002");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed = parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes())
        .expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("日旦"),
        "ToUnicode bfrange must decode sequential CID hex text; actual: {:?}",
        parsed.text_preview
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_tounicode_bfrange_array_hex_text() {
    let temp_file = temp_file_path("tounicode_bfrange_array_hex_text_preview_test");
    let pdf_bytes = minimal_pdf_with_tounicode_bfrange_array_text("00010002");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed = parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes())
        .expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("日本"),
        "ToUnicode bfrange array must decode explicit destinations; actual: {:?}",
        parsed.text_preview
    );
    assert_eq!(parsed.text_elements[0].text, "日本");

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_tounicode_one_byte_cid_hex_text() {
    let temp_file = temp_file_path("tounicode_one_byte_cid_hex_text_test");
    let pdf_bytes = minimal_pdf_with_tounicode_one_byte_bfchar_text("0102");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed = parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes())
        .expect("minimal pdf must be parsed");

    assert!(
        parsed.text_preview.contains("AB"),
        "ToUnicode CMap must split hex text using 1 byte CID width; actual: {:?}",
        parsed.text_preview
    );
    assert_eq!(parsed.text_elements[0].text, "AB");

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_tounicode_mixed_width_cid_hex_text_by_longest_match() {
    let temp_file = temp_file_path("tounicode_mixed_width_cid_hex_text_test");
    let pdf_bytes = minimal_pdf_with_tounicode_mixed_width_bfchar_text("01800102");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed = parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes())
        .expect("minimal pdf must be parsed");

    assert!(
        parsed.text_preview.contains("A日B"),
        "ToUnicode CMap must split mixed-width CID text by longest match; actual: {:?}",
        parsed.text_preview
    );
    assert_eq!(parsed.text_elements[0].text, "A日B");

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_does_not_decode_cid_width_disallowed_by_codespacerange() {
    let temp_file = temp_file_path("tounicode_disallowed_width_cid_hex_text_test");
    let pdf_bytes = minimal_pdf_with_tounicode_disallowed_width_bfchar_text("8001");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed = parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes())
        .expect("minimal pdf must be parsed");

    assert!(
        !parsed.text_preview.contains("日"),
        "ToUnicode CMap must not use mapping widths outside codespacerange; actual: {:?}",
        parsed.text_preview
    );
    assert_ne!(parsed.text_elements[0].text, "日");

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_does_not_decode_cid_value_disallowed_by_codespacerange() {
    let temp_file = temp_file_path("tounicode_disallowed_range_cid_hex_text_test");
    let pdf_bytes = minimal_pdf_with_tounicode_disallowed_range_bfchar_text("80");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed = parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes())
        .expect("minimal pdf must be parsed");

    assert!(
        !parsed.text_preview.contains("日"),
        "ToUnicode CMap must not use mappings outside codespacerange value bounds; actual: {:?}",
        parsed.text_preview
    );
    assert_ne!(parsed.text_elements[0].text, "日");

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_preserves_fragment_order_between_hex_and_literal() {
    let temp_file = temp_file_path("fragment_order_test");
    // <FEFF65E5672C> は "日本"
    let pdf = minimal_pdf_with_ordered_hex_and_literal("FEFF65E5672C", "Hello");

    let parsed = parse_pdf_bytes(&temp_file, pdf.as_bytes()).expect("minimal pdf must be parsed");
    let jp_pos = parsed
        .text_preview
        .find("日本")
        .expect("preview must include Japanese text");
    let hello_pos = parsed
        .text_preview
        .find("Hello")
        .expect("preview must include literal text");
    assert!(jp_pos < hello_pos, "hex fragment must keep source order");

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_extracts_fragments_inside_tj_array() {
    let temp_file = temp_file_path("tj_array_text_preview_test");
    let pdf = minimal_pdf_with_tj_array("FEFF65E5672C", "Hello", "20544444");

    let parsed = parse_pdf_bytes(&temp_file, pdf.as_bytes()).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("日本Hello TDD"),
        "TJ array fragments must be concatenated in order"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_extracts_mixed_escaped_octal_and_hex_in_tj_array() {
    let temp_file = temp_file_path("tj_array_mixed_escaped_octal_hex_test");
    // literal 内の `\(` `\)` `\050` `\051` と hex を同時に復号できることを検証する。
    fs::write(
        &temp_file,
        minimal_pdf_with_tj_array("48656C6C6F", "A\\(B\\)\\050C\\051", "20544444"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("HelloA(B)(C) TDD"),
        "mixed escaped/octal/hex fragments in TJ array must be concatenated in order"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_extracts_line_continuation_in_tj_array_literal() {
    let temp_file = temp_file_path("tj_array_line_continuation_literal_test");
    // literal 内の `\` + LF が行継続として解釈され、hex 断片と順序を保って連結されることを検証する。
    fs::write(
        &temp_file,
        minimal_pdf_with_tj_array("48656C6C6F", "A\\\nB", "20544444"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("HelloAB TDD"),
        "line continuation in TJ literal must be collapsed before concatenation"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_ignores_exponent_numeric_tokens_in_tj_array() {
    let temp_file = temp_file_path("tj_array_exponent_numeric_tokens_test");
    fs::write(
        &temp_file,
        minimal_pdf_with_tj_array_numeric_tokens("Hello", "World", "1e2", "-3.4E-2"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("HelloWorld"),
        "exponent-style numeric kerning tokens should be ignored in TJ array"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_ignores_exponent_numeric_tokens_with_utf16_hex_in_tj_array() {
    let temp_file = temp_file_path("tj_array_exponent_utf16_hex_tokens_test");
    let pdf = minimal_pdf_with_tj_array_utf16_and_numeric_tokens("A", "B", "1e2", "-3.4E-2");

    let parsed = parse_pdf_bytes(&temp_file, pdf.as_bytes()).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("A日本B"),
        "numeric exponent tokens should be ignored even with utf16 hex fragment"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_decodes_nested_parentheses_in_tj_array_literal() {
    let temp_file = temp_file_path("tj_array_nested_parentheses_literal_test");
    fs::write(
        &temp_file,
        minimal_pdf_with_tj_array("48656C6C6F", "A(B(C)D)E", "20544444"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("HelloA(B(C)D)E TDD"),
        "nested parentheses in TJ literal must be preserved with neighboring hex fragments"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_bytes_extracts_positioned_text_from_uncompressed_literal_tj() {
    let temp_file = temp_file_path("uncompressed_literal_tj_text_element_test");
    let pdf_bytes = minimal_pdf_with_literal_text("PlainElement");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed =
        parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes()).expect("parse must succeed");

    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].page, 0);
    assert_eq!(parsed.text_elements[0].text, "PlainElement");
    assert_eq!(parsed.text_elements[0].font_size, 12.0);
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert_eq!(parsed.text_elements[0].bounds.x0, 10.0);
    assert_eq!(parsed.text_elements[0].bounds.y0, 100.0);

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_bytes_extracts_positioned_text_from_uncompressed_tj_array_and_hex() {
    let temp_file = temp_file_path("uncompressed_tj_array_text_element_test");
    let pdf_bytes = minimal_pdf_with_tj_array("48656C6C6F", "A\\(B\\)", "20544444");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed =
        parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes()).expect("parse must succeed");

    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "HelloA(B) TDD");
    assert_eq!(parsed.text_elements[0].bounds.x0, 10.0);
    assert_eq!(parsed.text_elements[0].bounds.y0, 100.0);

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_bytes_applies_tounicode_cmap_to_positioned_text_elements() {
    let temp_file = temp_file_path("tounicode_text_element_test");
    let pdf_bytes = minimal_pdf_with_tounicode_bfchar_text("00010001");
    fs::write(&temp_file, &pdf_bytes).expect("failed to create minimal pdf");

    let parsed =
        parse_pdf_bytes(temp_file.clone(), pdf_bytes.as_bytes()).expect("parse must succeed");

    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(
        parsed.text_elements[0].text, "日日",
        "ToUnicode CMap must be applied to positioned text elements"
    );
    assert_eq!(parsed.text_elements[0].bounds.x0, 10.0);
    assert_eq!(parsed.text_elements[0].bounds.y0, 100.0);

    let _ = fs::remove_file(temp_file);
}
