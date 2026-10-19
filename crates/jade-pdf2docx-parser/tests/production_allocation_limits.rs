//! 共通確保容量の超過が公開APIで成功や品質置換に変わらないことを検証する。
use jade_pdf2docx_parser::{
    ParseError, ParseLimits, ParseOptions, parse_pdf_bytes_with_options, parse_pdf_with_options,
};

#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// 正式xrefとHelvetica本文を持つ一ページの入力を作る。
fn pdf() -> Vec<u8> {
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".into(),
        fixture::stream("BT /F1 12 Tf 10 80 Td (allocation boundary) Tj ET"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 容量だけを変更し、個別用途の既定予算は維持する。
fn options(limit: u64, strict: bool) -> ParseOptions {
    ParseOptions {
        limits: ParseLimits {
            max_total_allocation_bytes: limit,
            ..Default::default()
        },
        strict,
        ..Default::default()
    }
}

/// byte/file・normal/strictとも予算ゼロで同じ明示的な資源超過になる。
#[test]
fn allocation_limit_is_not_a_quality_fallback() {
    let bytes = pdf();
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-allocation-limit-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, &bytes).unwrap();
    for strict in [false, true] {
        let options = options(0, strict);
        for result in [
            parse_pdf_bytes_with_options("input.pdf", &bytes, &options),
            parse_pdf_with_options(&path, &options),
        ] {
            assert!(matches!(result, Err(ParseError::ResourceLimit {
                resource: "total allocation bytes", limit: 0, actual,
            }) if actual > 0));
        }
    }
    std::fs::remove_file(&path).unwrap();
    assert!(parse_pdf_bytes_with_options("input.pdf", &bytes, &options(u64::MAX, true)).is_ok());
}

/// 実際の処理全体のinclusive境界を探し、1 byte不足を同じ公開エラーにする。
#[test]
fn complete_parse_has_an_inclusive_allocation_boundary() {
    let bytes = pdf();
    let parse = |limit| parse_pdf_bytes_with_options("input.pdf", &bytes, &options(limit, true));
    let mut low = 0;
    let mut high = 1024 * 1024;
    assert!(parse(high).is_ok());
    while low < high {
        let middle = low + (high - low) / 2;
        if parse(middle).is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    assert!(low > 0);
    let exact = parse(low).unwrap();
    assert!(exact.text_preview.contains("allocation boundary"));
    assert!(matches!(parse(low - 1), Err(ParseError::ResourceLimit {
        resource: "total allocation bytes", actual, ..
    }) if actual == low));
    assert_eq!(parse(low).unwrap().text_preview, exact.text_preview);
}

/// file入力の保存容量をheader検査前に予約し、借用bytesの入口では数えない。
#[test]
fn file_input_is_counted_before_header_validation() {
    let bytes = b"invalid";
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-input-allocation-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    for strict in [false, true] {
        assert!(matches!(
            parse_pdf_with_options(&path, &options(6, strict)),
            Err(ParseError::ResourceLimit {
                resource: "total allocation bytes",
                limit: 6,
                actual: 7,
            })
        ));
        assert!(matches!(
            parse_pdf_with_options(&path, &options(7, strict)),
            Err(ParseError::InvalidPdfHeader { .. })
        ));
        assert!(matches!(
            parse_pdf_bytes_with_options("input.pdf", bytes, &options(0, strict)),
            Err(ParseError::InvalidPdfHeader { .. })
        ));
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::remove_file(path).unwrap();
}
