//! 所有のType1メタデータで、font programの組み込みEncodingとPDF側の上書きを分離する。
//! Font描画用CharStringsは含まず、Unicode抽出と品質判定だけを検証する。
use jade_pdf2docx_parser::{ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
use proptest::prelude::*;

#[path = "support/type1_builtin_pdf.rs"]
mod fixture;
use fixture::pdf;

/// AGLの既知glyph名は、元codeをUnicodeへ直結せず組み込み表に従って読む。
#[test]
fn known_builtin_names_recover_authored_unicode() {
    let bytes = pdf("256 array dup 65 /B put", "", false, "F1", "A");
    let parsed =
        parse_pdf_bytes_with_options("known-builtin.pdf", &bytes, &ParseOptions::default())
            .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "B"
    );
    assert!(parsed.quality.issues.is_empty());
}

/// BaseEncodingのないDifferencesは、他のcodeの組み込み対応を初期化しない。
#[test]
fn differences_override_only_their_builtin_slots() {
    let bytes = pdf(
        "256 array dup 65 /B put dup 66 /C put",
        "/Encoding << /Differences [66 /D] >>",
        false,
        "F1",
        "AB",
    );
    let parsed =
        parse_pdf_bytes_with_options("builtin-differences.pdf", &bytes, &ParseOptions::default())
            .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "BD"
    );
    assert!(parsed.quality.issues.is_empty());
}

/// 不明な組み込みglyph名を、無警告でraw ASCII本文へ置換しない。
#[test]
fn opaque_builtin_glyph_names_report_the_painted_font() {
    for encoding in ["", "/Encoding << /Differences [0 /G00] >>"] {
        let bytes = pdf("256 array dup 65 /G41 put", encoding, false, "F1", "A");
        let parsed =
            parse_pdf_bytes_with_options("opaque-builtin.pdf", &bytes, &ParseOptions::default())
                .unwrap();
        assert!(
            parsed.quality.issues.iter().any(|issue| matches!(
                issue,
                QualityIssue::UnsupportedFont {
                    page: 0,
                    object_number: Some(5),
                    ..
                }
            )),
            "{:?}",
            parsed.quality
        );
    }
}

/// 未使用の不明slotが同じ表にあっても、描画した既知glyphは正しく復元する。
#[test]
fn unused_opaque_slot_does_not_reject_a_known_glyph() {
    let bytes = pdf(
        "256 array dup 65 /B put dup 66 /G42 put",
        "",
        false,
        "F1",
        "A",
    );
    let parsed = parse_pdf_bytes_with_options(
        "mixed-builtin.pdf",
        &bytes,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "B"
    );
    assert!(parsed.quality.issues.is_empty());
}

/// 一部のToUnicodeがあっても、描画した別codeの未知glyphを検証済みにはしない。
#[test]
fn partial_tounicode_does_not_hide_an_opaque_painted_glyph() {
    let bytes = pdf(
        "256 array dup 65 /G41 put dup 66 /G42 put",
        "",
        true,
        "F1",
        "AB",
    );
    let parsed =
        parse_pdf_bytes_with_options("partial-builtin.pdf", &bytes, &ParseOptions::default())
            .unwrap();
    assert!(
        parsed.quality.issues.iter().any(|issue| matches!(
            issue,
            QualityIssue::UnsupportedFont {
                page: 0,
                object_number: Some(5),
                ..
            }
        )),
        "{:?}",
        parsed.quality
    );
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "strict-partial-builtin.pdf",
            &bytes,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}

/// control codeだけのDifferencesでも、本文の未知Encodingはstrict拒否する。
#[test]
fn strict_rejects_unverified_builtin_encoding() {
    let bytes = pdf(
        "256 array dup 65 /G41 put",
        "/Encoding << /Differences [0 /G00] >>",
        false,
        "F1",
        "A",
    );
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "strict-opaque.pdf",
            &bytes,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}

/// 完全なToUnicode対応があれば、glyph名が独自でもUnicodeを再推測しない。
#[test]
fn complete_tounicode_overrides_opaque_builtin_names() {
    let bytes = pdf("256 array dup 65 /G41 put", "", true, "F1", "A");
    let parsed = parse_pdf_bytes_with_options(
        "unicode-builtin.pdf",
        &bytes,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "Z"
    );
    assert!(parsed.quality.issues.is_empty());
}

/// PDFが明示する既知BaseEncodingは、font programの組み込み表より優先する。
#[test]
fn named_encoding_overrides_the_font_program() {
    let bytes = pdf(
        "256 array dup 65 /B put",
        "/Encoding /WinAnsiEncoding",
        false,
        "F1",
        "A",
    );
    let parsed = parse_pdf_bytes_with_options(
        "explicit-builtin.pdf",
        &bytes,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "A"
    );
}

/// 宣言だけの不明fontは、別fontで描いたページに品質欠落を追加しない。
#[test]
fn unused_opaque_builtin_font_is_not_reported() {
    let bytes = pdf("256 array dup 65 /G41 put", "", false, "F2", "A");
    let parsed = parse_pdf_bytes_with_options(
        "unused-builtin.pdf",
        &bytes,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(parsed.quality.issues.is_empty());
}

/// Type1の標準組み込みEncodingを、全て不明という扱いに広げない。
#[test]
fn standard_builtin_encoding_remains_supported() {
    let bytes = pdf("StandardEncoding", "", false, "F1", "Plain");
    let parsed = parse_pdf_bytes_with_options(
        "standard-builtin.pdf",
        &bytes,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "Plain"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 入力codeの番号と、作者が指定した一字のUnicode対応を独立に変える。
    #[test]
    fn builtin_glyph_assignment_controls_unicode(input in b'A'..=b'Z', mapped in b'A'..=b'Z') {
        prop_assume!(input != mapped);
        let builtin = format!("256 array dup {input} /{} put", mapped as char);
        let bytes = pdf(&builtin, "", false, "F1", &(input as char).to_string());
        let parsed = parse_pdf_bytes_with_options("builtin-property.pdf", &bytes, &ParseOptions::default()).unwrap();
        prop_assert_eq!(parsed.text_elements.iter().map(|text| text.text.as_str()).collect::<String>(), (mapped as char).to_string());
        prop_assert!(parsed.quality.issues.is_empty());
    }
}

/// PDFのDifferences名のescapeを先に復号し、AGLのsuffix規則へ渡す。
#[test]
fn escaped_pdf_difference_name_keeps_its_glyph_semantics() {
    let bytes = pdf(
        "256 array dup 65 /C put",
        "/Encoding << /Differences [65 /B#2Esalt] >>",
        false,
        "F1",
        "A",
    );
    let parsed = parse_pdf_bytes_with_options(
        "escaped-difference.pdf",
        &bytes,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "B"
    );
    assert!(parsed.quality.issues.is_empty());
}

/// PostScript内の#はPDF name escapeではなく、不明名を勝手に復号しない。
#[test]
fn postscript_glyph_names_do_not_use_pdf_name_escaping() {
    let bytes = pdf("256 array dup 65 /B#2Esalt put", "", false, "F1", "A");
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "literal-postscript-name.pdf",
            &bytes,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}

/// StandardEncodingのquote・ligatureを、ASCIIやLatin1の同一codeに取り違えない。
#[test]
fn standard_encoding_keeps_curly_quotes_and_ligature() {
    let bytes = pdf("StandardEncoding", "", false, "F1", r"\047\140\256");
    let parsed = parse_pdf_bytes_with_options(
        "standard-quotes.pdf",
        &bytes,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "\u{2019}\u{2018}\u{fb01}"
    );
}

/// 部分ToUnicodeの既知codeを保ち、別codeは組み込みglyph名で補完する。
#[test]
fn partial_tounicode_preserves_known_builtin_fallback() {
    let bytes = pdf(
        "256 array dup 65 /B put dup 66 /C put",
        "",
        true,
        "F1",
        "AB",
    );
    let parsed = parse_pdf_bytes_with_options(
        "partial-known-builtin.pdf",
        &bytes,
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>(),
        "ZC"
    );
}
