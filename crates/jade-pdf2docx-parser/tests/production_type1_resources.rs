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

/// 誤った世代のFontDescriptor・FontFile・Encodingでraw ASCIIへ黙って戻らない。
#[test]
fn stale_font_references_cannot_bypass_strict_encoding_checks() {
    for (object, before, after) in [
        (4, "/FontDescriptor 6 0 R", "/FontDescriptor 6 1 R"),
        (5, "/FontFile 7 0 R", "/FontFile 7 1 R"),
        (
            4,
            "/BaseFont /OwnedBuiltin",
            "/BaseFont /OwnedBuiltin /Encoding 10 1 R",
        ),
    ] {
        let mut objects = objects("256 array dup 65 /B put", "", false, "F1", "A");
        objects[object] = String::from_utf8(objects[object].clone())
            .unwrap()
            .replace(before, after)
            .into_bytes();
        objects.push(b"<< /Differences [65 /C] >>".to_vec());
        assert!(
            parse_pdf_bytes_with_options(
                "stale-type1.pdf",
                &serialize(&objects),
                &ParseOptions {
                    strict: true,
                    ..Default::default()
                }
            )
            .is_err(),
            "{after}"
        );
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
