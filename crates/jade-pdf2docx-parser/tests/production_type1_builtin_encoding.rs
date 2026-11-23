//! 所有のType1メタデータで、font programの組み込みEncodingとPDF側の上書きを分離する。
//! Font描画用CharStringsは含まず、Unicode抽出と品質判定だけを検証する。
use jade_pdf2docx_parser::{ParseError, ParseOptions, QualityIssue, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// 正確なLengthを持つ、所有の非圧縮メタデータstreamを作る。
fn stream(data: &str, attributes: &str) -> Vec<u8> {
    format!(
        "<< /Length {} {attributes} >>\nstream\n{data}\nendstream",
        data.len()
    )
    .into_bytes()
}

/// 埋め込みType1のEncodingだけを持つメタデータcontrolを作り、描画字体の妥当性とは分離する。
fn program(encoding: &str) -> String {
    format!(
        "%!PS-AdobeFont-1.0: OwnedBuiltin 1.0\n11 dict begin\n/FontName /OwnedBuiltin def\n/FontType 1 def\n/FontBBox [0 0 600 700] def\n/FontMatrix [0.001 0 0 0.001 0 0] def\n/Encoding {encoding} readonly def\ncurrentdict end\ncurrentfile eexec\n"
    )
}

/// PDF側Encoding・ToUnicodeと、描画するfontを独立に指定できる正式xref付きPDF。
fn pdf(builtin: &str, encoding: &str, unicode: bool, selected: &str, text: &str) -> Vec<u8> {
    let font_program = program(builtin);
    let mapping = if unicode { "/ToUnicode 8 0 R" } else { "" };
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 9 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(&format!("BT /{selected} 12 Tf 72 700 Td ({text}) Tj ET"), ""),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /OwnedBuiltin /FontDescriptor 6 0 R {encoding} {mapping} >>").into_bytes(),
        b"<< /Type /FontDescriptor /FontName /OwnedBuiltin /Flags 32 /FontBBox [0 0 600 700] /Ascent 700 /Descent -200 /CapHeight 700 /StemV 80 /FontFile 7 0 R >>".to_vec(),
        stream(&font_program, &format!("/Length1 {} /Length2 0 /Length3 0", font_program.len())),
        stream("begincmap 1 begincodespacerange <00> <ff> endcodespacerange 1 beginbfchar <41> <005a> endbfchar endcmap", ""),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        data.extend_from_slice(object);
        data.extend_from_slice(b"\nendobj\n");
    }
    let xref = data.len();
    data.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    data
}

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
