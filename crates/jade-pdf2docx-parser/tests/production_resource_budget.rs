//! Resource/Font cache の容量超過を公開入口で返す回帰。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes_with_limits};
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 間接 Font の別名と二ページを持つ、幅表 payload のない PDF を作る。
fn shared_font_pdf() -> Vec<u8> {
    let mut objects =
        pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R /Alias 10 0 R >> >>");
    objects[4] = stream("BT /F 10 Tf 10 10 Td (A) Tj ET");
    objects[5] = stream("BT /Alias 10 Tf 10 10 Td (B) Tj ET");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    pdf(&objects)
}

/// 全解析段階で累積した要求量の境界を検証し、再解析で予算が独立することを固定する。
#[test]
fn cumulative_resource_capacity_is_inclusive_at_public_api() {
    let bytes = shared_font_pdf();
    let mut capacity = 0;
    let mut completed = false;
    for _ in 0..64 {
        match parse_pdf_bytes_with_limits(
            "resource-budget.pdf",
            &bytes,
            &ParseLimits {
                max_total_resource_workspace_bytes: capacity,
                ..Default::default()
            },
        ) {
            Err(ParseError::ResourceLimit {
                resource: "resource workspace bytes",
                limit,
                actual,
            }) => {
                assert_eq!(limit, capacity);
                assert!(actual > capacity);
                capacity = actual;
            }
            Ok(parsed) => {
                assert!(capacity > 0);
                assert_eq!(parsed.text_elements.len(), 2);
                assert_eq!(parsed.text_elements[0].text, "A");
                assert_eq!(parsed.text_elements[1].text, "B");
                completed = true;
                break;
            }
            result => panic!("unexpected {result:?}"),
        }
    }
    assert!(completed, "累積予算の境界へ到達しない");
    for _ in 0..2 {
        assert!(
            matches!(parse_pdf_bytes_with_limits("resource-budget.pdf", &bytes, &ParseLimits {
            max_total_resource_workspace_bytes: capacity - 1, ..Default::default()
        }), Err(ParseError::ResourceLimit { resource: "resource workspace bytes", actual, .. }) if actual == capacity)
        );
        assert!(
            parse_pdf_bytes_with_limits(
                "resource-budget.pdf",
                &bytes,
                &ParseLimits {
                    max_total_resource_workspace_bytes: capacity,
                    ..Default::default()
                }
            )
            .is_ok()
        );
    }
}
