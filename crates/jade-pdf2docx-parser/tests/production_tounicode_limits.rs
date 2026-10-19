//! 公開 API で ToUnicode の処理上限と不正範囲を隠さず返す回帰。
use jade_pdf2docx_parser::{
    ParseError, ParseLimits, ParsedDocument, parse_pdf_bytes_with_limits, parse_pdf_with_limits,
};

#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// ページまたは Form の Font に指定 CMap を持たせる正式な PDF を生成する。
fn document(cmap: &str, in_form: bool) -> Vec<u8> {
    let font = "/Font << /F 10 0 R >>";
    let mut objects = fixture::pages(&format!(
        "/MediaBox [0 0 100 100] /Resources << {} >>",
        if in_form {
            "/XObject << /Fm 7 0 R >>"
        } else {
            font
        }
    ));
    let content = "BT /F 10 Tf 10 10 Td <000100020003> Tj ET";
    objects[4] = fixture::stream(if in_form { "/Fm Do" } else { content });
    objects[5] = fixture::stream("");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << {font} >> /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push("<< /Type /Font /Subtype /Type0 /BaseFont /Custom /Encoding /Identity-H /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>".into());
    objects.push("<< /Type /Font /Subtype /CIDFontType2 /DW 600 >>".into());
    objects.push(fixture::stream(cmap));
    fixture::pdf(&objects)
}

/// 処理数だけを上書きした公開 API で解析する。
fn parse(cmap: &str, limit: u64, in_form: bool) -> Result<ParsedDocument, ParseError> {
    parse_pdf_bytes_with_limits(
        "cmap-budget.pdf",
        &document(cmap, in_form),
        &ParseLimits {
            max_total_tounicode_entries: limit,
            ..Default::default()
        },
    )
}

/// ページ・Form・preview の繰り返し抽出でも、一回分の entry 予算で本文を保持する。
#[test]
fn page_and_form_reuse_cmap_with_single_parse_budget() {
    for in_form in [false, true] {
        let parsed = parse(
            "1 beginbfrange\n<0001> <0003> <0041>\nendbfrange",
            3,
            in_form,
        )
        .unwrap();
        assert!(
            parsed
                .text_elements
                .iter()
                .any(|element| element.text == "ABC")
        );
    }
}

/// 同一文書の異なるページ・Form Font が同じ CMap を一度だけ解析する。
#[test]
fn distinct_page_and_form_fonts_reuse_same_stream() {
    let font = "/Font << /F 10 0 R >>";
    let mut objects = fixture::pages(&format!(
        "/MediaBox [0 0 100 100] /Resources << {font} /XObject << /Fm 7 0 R >> >>"
    ));
    objects[4] = fixture::stream("BT /F 10 Tf 10 10 Td <0001> Tj ET /Fm Do");
    objects[5] = fixture::stream("");
    let content = "BT /G 10 Tf 10 30 Td <0001> Tj ET";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /Font << /G 13 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    let font_dict = "<< /Type /Font /Subtype /Type0 /BaseFont /Custom /Encoding /Identity-H /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>";
    objects.push(font_dict.into());
    objects.push("<< /Type /Font /Subtype /CIDFontType2 /DW 600 >>".into());
    objects.push(fixture::stream("1 beginbfchar\n<0001> <0041>\nendbfchar"));
    objects.push(font_dict.into());
    let parsed = parse_pdf_bytes_with_limits(
        "shared-page-form.pdf",
        &fixture::pdf(&objects),
        &ParseLimits {
            max_total_tounicode_entries: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .filter(|element| element.text == "A")
            .count(),
        2
    );
}

/// ページと Form の Font で、範囲の超過を推定文字列や native 回復で隠さない。
#[test]
fn page_and_form_range_excess_is_explicit() {
    for in_form in [false, true] {
        let result = parse(
            "1 beginbfrange\n<0001> <0003> <0041>\nendbfrange",
            2,
            in_form,
        );
        assert!(
            matches!(
                result,
                Err(ParseError::ResourceLimit {
                    resource: "ToUnicode entries",
                    limit: 2,
                    actual: 3
                })
            ),
            "{result:?}"
        );
    }
}

/// 既定上限でも全 u32 範囲を reject し、cardinality の overflow を避ける。
#[test]
fn default_limit_rejects_full_source_range() {
    let source = document(
        "1 beginbfrange\n<00000000> <FFFFFFFF> <00000000>\nendbfrange",
        false,
    );
    let result = parse_pdf_bytes_with_limits("huge-cmap.pdf", &source, &ParseLimits::default());
    assert!(
        matches!(
            result,
            Err(ParseError::ResourceLimit {
                resource: "ToUnicode entries",
                limit: 1_000_000,
                actual: 4_294_967_296
            })
        ),
        "{result:?}"
    );
}

/// destination の演算 overflow は bytes API で型付きエラーとして観測できる。
#[test]
fn destination_overflow_reaches_public_api() {
    let result = parse(
        "1 beginbfrange\n<0001> <0002> <FFFFFFFF>\nendbfrange",
        64,
        false,
    );
    assert!(
        matches!(
            result,
            Err(ParseError::InvalidToUnicode {
                reason: "bfrange destination exceeds u32"
            })
        ),
        "{result:?}"
    );
}

/// 失敗した解析とは独立した予算で有効 CMap の本文を保持する。
#[test]
fn failure_does_not_poison_subsequent_parse() {
    let cmap = "1 beginbfrange\n<0001> <0003> <0041>\nendbfrange";
    assert!(parse(cmap, 0, false).is_err());
    for in_form in [false, true] {
        let parsed = parse(cmap, 64, in_form).unwrap();
        assert!(
            parsed
                .text_elements
                .iter()
                .any(|element| element.text == "ABC"),
            "{:?}",
            parsed.text_elements
        );
    }
}

/// file API も native backend の前に同じ処理上限を返す。
#[test]
fn file_api_returns_cmap_limit() {
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-cmap-budget-{}.pdf",
        std::process::id()
    ));
    std::fs::write(
        &path,
        document("1 beginbfchar\n<0001> <0041>\nendbfchar", false),
    )
    .unwrap();
    let result = parse_pdf_with_limits(
        &path,
        &ParseLimits {
            max_total_tounicode_entries: 0,
            ..Default::default()
        },
    );
    std::fs::remove_file(path).unwrap();
    assert!(
        matches!(
            result,
            Err(ParseError::ResourceLimit {
                resource: "ToUnicode entries",
                limit: 0,
                actual: 1
            })
        ),
        "{result:?}"
    );
}

/// ページ/Form の可変長 payload 超過も回復処理で隠さず、別解析へ持ち越さない。
#[test]
fn page_and_form_mapping_bytes_reach_public_api() {
    let cmap = "1 beginbfrange\n<0001> <0003> <0041>\nendbfrange";
    for in_form in [false, true] {
        let bytes = document(cmap, in_form);
        let result = parse_pdf_bytes_with_limits(
            "payload.pdf",
            &bytes,
            &ParseLimits {
                max_total_tounicode_bytes: 4,
                ..Default::default()
            },
        );
        assert!(
            matches!(
                result,
                Err(ParseError::ResourceLimit {
                    resource: "ToUnicode mapping bytes",
                    limit: 4,
                    actual: 5,
                })
            ),
            "{result:?}"
        );
        let parsed =
            parse_pdf_bytes_with_limits("fresh.pdf", &bytes, &ParseLimits::default()).unwrap();
        assert!(
            parsed
                .text_elements
                .iter()
                .any(|element| element.text == "ABC")
        );
    }
}

/// file API でも mapping の String 確保前の超過を返す。
#[test]
fn file_api_returns_mapping_byte_limit() {
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-cmap-bytes-budget-{}.pdf",
        std::process::id()
    ));
    std::fs::write(
        &path,
        document("1 beginbfchar\n<0001> <0041>\nendbfchar", false),
    )
    .unwrap();
    let result = parse_pdf_with_limits(
        &path,
        &ParseLimits {
            max_total_tounicode_bytes: 0,
            ..Default::default()
        },
    );
    std::fs::remove_file(path).unwrap();
    assert!(
        matches!(
            result,
            Err(ParseError::ResourceLimit {
                resource: "ToUnicode mapping bytes",
                limit: 0,
                actual: 5,
            })
        ),
        "{result:?}"
    );
}

/// lookup 保存容量の超過もページ/Form の本文復号や native 回復で隠さない。
#[test]
fn page_and_form_storage_limit_reaches_public_api() {
    let cmap = "1 beginbfrange\n<0001> <0003> <0041>\nendbfrange";
    for in_form in [false, true] {
        let bytes = document(cmap, in_form);
        let result = parse_pdf_bytes_with_limits(
            "storage.pdf",
            &bytes,
            &ParseLimits {
                max_total_tounicode_storage_bytes: 0,
                ..Default::default()
            },
        );
        assert!(
            matches!(result, Err(ParseError::ResourceLimit {
            resource: "ToUnicode storage bytes", limit: 0, actual
        }) if actual == std::mem::size_of::<(String, String, usize)>() as u64),
            "{result:?}"
        );
        let fresh =
            parse_pdf_bytes_with_limits("fresh.pdf", &bytes, &ParseLimits::default()).unwrap();
        assert!(
            fresh
                .text_elements
                .iter()
                .any(|element| element.text == "ABC")
        );
    }
}

/// file API は lookup 保存容量の超過を native へ渡す前に明示する。
#[test]
fn file_api_returns_storage_limit() {
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-cmap-storage-budget-{}.pdf",
        std::process::id()
    ));
    std::fs::write(
        &path,
        document("1 beginbfchar\n<0001> <0041>\nendbfchar", false),
    )
    .unwrap();
    let result = parse_pdf_with_limits(
        &path,
        &ParseLimits {
            max_total_tounicode_storage_bytes: 0,
            ..Default::default()
        },
    );
    std::fs::remove_file(path).unwrap();
    assert!(
        matches!(result, Err(ParseError::ResourceLimit {
        resource: "ToUnicode storage bytes", limit: 0, actual
    }) if actual == std::mem::size_of::<(String, String, usize)>() as u64),
        "{result:?}"
    );
}

/// mapping がない巨大な CMap もページ/Form の公開 API から入力上限を返す。
#[test]
fn empty_page_and_form_cmaps_have_input_limit() {
    let cmap = format!("0 beginbfchar\n{}\nendbfchar", " ".repeat(8192));
    for in_form in [false, true] {
        let input = document(&cmap, in_form);
        let result = parse_pdf_bytes_with_limits(
            "empty-cmap.pdf",
            &input,
            &ParseLimits {
                max_total_tounicode_input_bytes: 8191,
                ..Default::default()
            },
        );
        assert!(
            matches!(result, Err(ParseError::ResourceLimit {
            resource: "ToUnicode input bytes", limit: 8191, actual
        }) if actual >= cmap.len() as u64),
            "{result:?}"
        );
        assert!(
            parse_pdf_bytes_with_limits("empty-cmap.pdf", &input, &ParseLimits::default()).is_ok()
        );
    }
}
