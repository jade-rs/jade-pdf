//! Type1の局所resource・世代・描画状態・共有予算を公開bytes APIで検証する。
use jade_pdf2docx_parser::{
    ParseError, ParseLimits, ParseOptions, QualityIssue, UnsupportedFontReason,
    parse_pdf_bytes_with_options,
};
#[path = "support/type1_builtin_pdf.rs"]
mod fixture;
use fixture::{objects, serialize, stream};

/// fontだけの診断を取り出し、別のForm再現性診断と混同しない。
fn font_issues(parsed: &jade_pdf2docx_parser::ParsedDocument) -> Vec<Option<u32>> {
    parsed
        .quality
        .issues
        .iter()
        .filter_map(|issue| match issue {
            QualityIssue::UnsupportedFont {
                object_number,
                reason: UnsupportedFontReason::Encoding,
                ..
            } => Some(*object_number),
            _ => None,
        })
        .collect()
}

/// 直接Font辞書にも間接Fontと同じ組み込み表を適用する。
#[test]
fn inline_font_uses_its_own_builtin_table() {
    let mut objects = objects("256 array dup 65 /B put", "", false, "F1", "A");
    let font = String::from_utf8(objects[4].clone()).unwrap();
    objects[2] = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font} >> >> /Contents 4 0 R >>").into_bytes();
    let parsed = parse_pdf_bytes_with_options(
        "inline-builtin.pdf",
        &serialize(&objects),
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
            .map(|element| element.text.as_str())
            .collect::<String>(),
        "B"
    );
    assert!(font_issues(&parsed).is_empty());
}

/// Font aliasの最終objectへUnicode表と診断identityを揃える。
#[test]
fn font_alias_keeps_the_final_font_identity() {
    for opaque in [false, true] {
        let mut objects = objects(
            if opaque {
                "256 array dup 65 /G41 put"
            } else {
                "256 array dup 65 /B put"
            },
            "",
            false,
            "F1",
            "A",
        );
        let font = std::mem::replace(&mut objects[4], b"10 0 R".to_vec());
        objects.push(font);
        let parsed = parse_pdf_bytes_with_options(
            "alias-builtin.pdf",
            &serialize(&objects),
            &ParseOptions::default(),
        )
        .unwrap();
        if opaque {
            assert_eq!(font_issues(&parsed), vec![Some(10)]);
        } else {
            assert!(font_issues(&parsed).is_empty());
            assert_eq!(
                parsed
                    .text_elements
                    .iter()
                    .map(|element| element.text.as_str())
                    .collect::<String>(),
                "B"
            );
        }
    }
}

/// 必要なFontDescriptor・FontFileを失った場合にraw ASCIIへ黙って戻らない。
#[test]
fn stale_font_references_cannot_bypass_strict_encoding_checks() {
    for (object, before, after) in [
        (4, "/FontDescriptor 6 0 R", "/FontDescriptor 6 1 R"),
        (5, "/FontFile 7 0 R", "/FontFile 7 1 R"),
    ] {
        let mut objects = objects("256 array dup 65 /B put", "", false, "F1", "A");
        objects[object] = String::from_utf8(objects[object].clone())
            .unwrap()
            .replace(before, after)
            .into_bytes();
        objects.push(b"<< /Differences [65 /C] >>".to_vec());
        let result = parse_pdf_bytes_with_options(
            "stale-type1.pdf",
            &serialize(&objects),
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
        );
        assert!(result.is_err(), "{after}: {result:?}");
    }
}

/// 任意のEncodingのnullと未定義参照は、仕様どおり省略と同じ組み込み表を使う。
#[test]
fn null_and_unresolved_optional_encoding_use_the_builtin_table() {
    for encoding in ["/Encoding null", "/Encoding 10 1 R"] {
        let mut objects = objects("256 array dup 65 /B put", encoding, false, "F1", "A");
        objects.push(b"<< /Differences [65 /C] >>".to_vec());
        let parsed = parse_pdf_bytes_with_options(
            "null-encoding.pdf",
            &serialize(&objects),
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
                .map(|element| element.text.as_str())
                .collect::<String>(),
            "B"
        );
    }
}

/// 独自fontで組み込み表そのものが無ければ、明示nullと省略の両方を確認不能とする。
#[test]
fn missing_custom_font_program_is_unverified() {
    for descriptor in ["", "/FontDescriptor null", "/FontDescriptor 6 1 R"] {
        let mut objects = objects("256 array dup 65 /B put", "", false, "F1", "A");
        objects[4] =
            format!("<< /Type /Font /Subtype /Type1 /BaseFont /OwnedBuiltin {descriptor} >>")
                .into_bytes();
        assert!(
            matches!(
                parse_pdf_bytes_with_options(
                    "missing-font.pdf",
                    &serialize(&objects),
                    &ParseOptions {
                        strict: true,
                        ..Default::default()
                    }
                ),
                Err(ParseError::QualityRejected { .. })
            ),
            "{descriptor}"
        );
    }
}

/// 標準14書体はFontDescriptorの省略・nullでも既存の対応経路を保つ。
#[test]
fn standard_font_with_optional_descriptor_remains_supported() {
    for descriptor in ["", "/FontDescriptor null", "/FontDescriptor 6 1 R"] {
        let mut objects = objects("256 array dup 65 /B put", "", false, "F1", "A");
        objects[4] = format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica {descriptor} >>")
            .into_bytes();
        let parsed = parse_pdf_bytes_with_options(
            "standard-font.pdf",
            &serialize(&objects),
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
                .map(|element| element.text.as_str())
                .collect::<String>(),
            "A"
        );
    }
}

/// font programがなくても完成ToUnicodeを共有し、未知code検査でentry予算を膨らませない。
#[test]
fn missing_program_preserves_existing_tounicode_budget_and_coverage() {
    // fixtureは一つのcode space範囲と一つのbfcharを持つため、既存処理量は二件。
    let control = fixture::pdf(
        "256 array dup 65 /G41 put",
        "/Encoding /WinAnsiEncoding",
        true,
        "F1",
        "A",
    );
    for limit in [1, 2] {
        let result = parse_pdf_bytes_with_options(
            "unicode-cost-control.pdf",
            &control,
            &ParseOptions {
                strict: true,
                limits: ParseLimits {
                    max_total_tounicode_entries: limit,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        if limit == 1 {
            assert!(matches!(
                result,
                Err(ParseError::ResourceLimit {
                    resource: "ToUnicode entries",
                    actual: 2,
                    ..
                })
            ));
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
    }
    for text in ["A", "AB"] {
        let mut objects = objects("256 array dup 65 /G41 put", "", true, "F1", text);
        objects[4] =
            b"<< /Type /Font /Subtype /Type1 /BaseFont /OwnedBuiltin /ToUnicode 8 0 R >>".to_vec();
        let result = parse_pdf_bytes_with_options(
            "missing-program-unicode.pdf",
            &serialize(&objects),
            &ParseOptions {
                strict: true,
                limits: ParseLimits {
                    max_total_tounicode_entries: 2,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        if text == "A" {
            let parsed = result.unwrap();
            assert_eq!(
                parsed
                    .text_elements
                    .iter()
                    .map(|element| element.text.as_str())
                    .collect::<String>(),
                "Z"
            );
        } else {
            assert!(
                matches!(result, Err(ParseError::QualityRejected { .. })),
                "{result:?}"
            );
        }
    }
}

/// Form内の同じresource名を、ページの別fontの組み込み表に取り違えない。
#[test]
fn form_local_font_overrides_the_same_page_resource_name() {
    let mut objects = objects("256 array dup 65 /B put", "", false, "F1", "A");
    objects[2] = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> /XObject << /Fm 10 0 R >> >> /Contents 4 0 R >>".to_vec();
    objects[3] = stream("BT /F1 12 Tf 72 700 Td (A) Tj ET /Fm Do", "");
    objects.push(stream(
        "BT /F1 12 Tf 72 600 Td (A) Tj ET",
        "/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /Font << /F1 9 0 R >> >>",
    ));
    let parsed = parse_pdf_bytes_with_options(
        "local-type1.pdf",
        &serialize(&objects),
        &ParseOptions::default(),
    )
    .unwrap();
    assert!(font_issues(&parsed).is_empty());
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|element| element.text.as_str())
            .collect::<String>(),
        "BA"
    );
}

/// FormにTfがなければ、呼び出し元で選んだ未知code表を保持する。
#[test]
fn form_without_tf_inherits_the_selected_encoding_coverage() {
    let mut objects = objects("256 array dup 65 /G41 put", "", false, "F1", "A");
    objects[2] = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> /XObject << /Fm 10 0 R >> >> /Contents 4 0 R >>".to_vec();
    objects[3] = stream("BT /F1 12 Tf ET /Fm Do", "");
    objects.push(stream(
        "BT 72 600 Td (A) Tj ET",
        "/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /Font << /F1 9 0 R >> >>",
    ));
    let parsed = parse_pdf_bytes_with_options(
        "inherited-type1.pdf",
        &serialize(&objects),
        &ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(font_issues(&parsed), vec![Some(5)]);
}

/// q/Qは選択Fontだけでなく、未知codeのcoverageも復元する。
#[test]
fn graphics_restore_keeps_the_selected_encoding_coverage() {
    let mut objects = objects("256 array dup 65 /G41 put", "", false, "F1", "A");
    objects[3] = stream("BT /F1 12 Tf q /F2 12 Tf (A) Tj Q (A) Tj ET", "");
    let parsed = parse_pdf_bytes_with_options(
        "restore-type1.pdf",
        &serialize(&objects),
        &ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(font_issues(&parsed), vec![Some(5)]);
}

/// 一つのFont表は抽出・診断で共有し、処理量上限の境界を保つ。
#[test]
fn repeated_extraction_and_diagnostics_share_one_byte_mapping_budget() {
    let bytes = fixture::pdf("256 array dup 65 /B put", "", false, "F1", "A");
    for limit in [255, 256] {
        let result = parse_pdf_bytes_with_options(
            "shared-type1.pdf",
            &bytes,
            &ParseOptions {
                strict: true,
                limits: ParseLimits {
                    max_total_tounicode_entries: limit,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        if limit == 256 {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(matches!(
                result,
                Err(ParseError::ResourceLimit {
                    resource: "ToUnicode entries",
                    limit: 255,
                    actual: 256
                })
            ));
        }
    }
}
