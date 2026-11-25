//! 所有TrueType辞書で、非記号fontの明示WinAnsiと原本ToUnicodeの優先順位を検証する。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

/// 非埋め込みの所有font辞書とbyte operandを生成し、外観の正しさは主張しない。
fn pdf(bytes: &[u8], route: usize, flags: &str, encoding: &str, unicode: bool) -> Vec<u8> {
    let literal = format!(
        "({})",
        bytes
            .iter()
            .map(|b| format!("\\{b:03o}"))
            .collect::<String>()
    );
    let hex = format!(
        "<{}>",
        bytes.iter().map(|b| format!("{b:02X}")).collect::<String>()
    );
    let show = match route {
        0 => format!("{literal} Tj"),
        1 => format!("{hex} Tj"),
        2 => format!("[{literal}] TJ"),
        3 => format!("[{hex}] TJ"),
        4 => format!("{literal} '"),
        5 => format!("0 0 {literal} \""),
        _ => unreachable!(),
    };
    let content = format!("BT /F1 12 Tf 72 700 Td 14 TL {show} ET");
    let cmap = "begincmap 1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <A7> <2603> endbfchar endcmap";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /TrueType /BaseFont /OwnedSans /Encoding {encoding} /FontDescriptor 6 0 R {} >>", if unicode { "/ToUnicode 7 0 R" } else { "" }),
        format!("<< /Type /FontDescriptor /FontName /OwnedSans /Flags {flags} >>"),
        format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len()),
    ];
    let mut data = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = data.len();
    data.extend_from_slice(b"xref\n0 8\n0000000000 65535 f \n");
    for offset in offsets {
        data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend_from_slice(
        format!("trailer << /Size 8 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    data
}

/// 純Rust公開byte parserの可視文字を、preview正規化なしに読む。
fn text(bytes: &[u8]) -> String {
    parse_pdf_bytes(std::path::Path::new("owned-truetype-winansi.pdf"), bytes)
        .unwrap()
        .text_elements
        .iter()
        .map(|e| e.text.as_str())
        .collect()
}

/// 非記号の明示WinAnsiは、全描画operandで節記号・通貨・Latin文字を保持する。
#[test]
fn nonsymbolic_truetype_named_winansi_retains_defined_unicode() {
    for flags in ["32", "96"] {
        for route in 0..6 {
            assert_eq!(
                text(&pdf(
                    &[0xa7, 0x80, 0xe9, 0x97],
                    route,
                    flags,
                    "/WinAnsiEncoding",
                    false
                )),
                "§€é—"
            );
        }
    }
}

/// 原本のToUnicodeをWinAnsiで上書きせず、異なる記号意味を維持する。
#[test]
fn authored_truetype_unicode_keeps_priority() {
    for route in 0..6 {
        assert_eq!(
            text(&pdf(&[0xa7], route, "32", "/WinAnsiEncoding", true)),
            "☃"
        );
    }
}

/// 記号・矛盾・不正flagと未対応encoding辞書を新しいUnicode対応へ昇格しない。
#[test]
fn unverified_truetype_flags_and_encodings_keep_legacy_fallback() {
    for flags in ["0", "4", "36", "-32", "32.0", "null"] {
        assert_ne!(
            text(&pdf(&[0xa7], 0, flags, "/WinAnsiEncoding", false)),
            "§"
        );
    }
    for encoding in [
        "/MacRomanEncoding",
        "null",
        "<< /BaseEncoding /WinAnsiEncoding /Differences [167 /snowman] >>",
    ] {
        assert_ne!(text(&pdf(&[0xa7], 0, "32", encoding, false)), "§");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {cases:24,failure_persistence:Some(Box::new(FileFailurePersistence::Direct("tests/production_truetype_winansi.proptest-regressions"))),..ProptestConfig::default()})]
    /// 既知の独立対応を任意の順番へ並べても、byte境界や命令に依存しない。
    #[test]
    fn truetype_winansi_sequences_keep_source_unicode(indices in prop::collection::vec(0usize..8,1..24),route in 0usize..6) {
        let pairs=[(0x41,"A"),(0xa7,"§"),(0x80,"€"),(0xe9,"é"),(0x97,"—"),(0xd7,"×"),(0x91,"‘"),(0x92,"’")];
        let mut bytes=Vec::new();let mut expected=String::new();
        for index in indices {bytes.push(pairs[index].0);expected.push_str(pairs[index].1);}
        prop_assert_eq!(text(&pdf(&bytes,route,"32","/WinAnsiEncoding",false)),expected);
    }
}

/// 未定義codeを有効な文字と誤認せず、予算と同様にstrictの失敗を維持する。
#[test]
fn undefined_truetype_winansi_slot_is_diagnosed_and_rejected_in_strict_mode() {
    use jade_pdf2docx_parser::{
        ParseError, ParseOptions, QualityIssue, UnsupportedFontReason, parse_pdf_bytes_with_options,
    };
    let bytes = pdf(&[b'A', 1, b'Z'], 0, "32", "/WinAnsiEncoding", false);
    let parsed = parse_pdf_bytes_with_options(
        "owned-undefined-truetype.pdf",
        &bytes,
        &ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|e| e.text.as_str())
            .collect::<String>(),
        "A�Z"
    );
    assert!(parsed.quality.issues.iter().any(|i| matches!(
        i,
        QualityIssue::UnsupportedFont {
            reason: UnsupportedFontReason::Encoding,
            ..
        }
    )));
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "owned-undefined-truetype.pdf",
            &bytes,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}
