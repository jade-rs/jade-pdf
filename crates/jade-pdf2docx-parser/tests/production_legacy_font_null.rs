//! Rootなし互換PDFのResources null継承を公開テキスト抽出で検査する。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};

/// 直接・間接Resourcesを選べるxrefなしPDFを作る。
fn pdf(resources: &str) -> Vec<u8> {
    pdf_with_content(resources, "BT /F 10 Tf 10 10 Td (AB) Tj ET")
}

/// 文字描画operandを変更しつつ正確なLengthを保持する。
fn pdf_with_content(resources: &str, content: &str) -> Vec<u8> {
    format!("%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 100 200] /CropBox [0 0 100 200] /Rotate 0 /Resources {resources} /Parent 2 0 R /Contents 3 0 R >> endobj\n2 0 obj << /Type /Pages /Resources << /Font << /F 4 0 R >> >> >> endobj\n3 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n4 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Courier >> endobj\n8 7 obj 9 3 R endobj\n9 3 obj null endobj\n10 7 obj << >> endobj\n%%EOF\n", content.len()).into_bytes()
}

/// 直接・間接nullから継承したFont名とglyph幅が公開出力に反映される。
#[test]
fn public_api_inherits_null_font_resources() {
    for resources in ["null", "8 7 R"] {
        let parsed = parse_pdf_bytes("null-resources.pdf", &pdf(resources)).unwrap();
        assert_eq!(parsed.text_elements.len(), 1);
        let text = &parsed.text_elements[0];
        assert_eq!(text.text, "AB");
        assert_eq!(text.font_name.as_deref(), Some("Courier"));
        assert_eq!(text.bounds.x1 - text.bounds.x0, 12.0);
    }
}

/// 空のResources辞書は親Fontを公開出力へ混入させない。
#[test]
fn public_api_empty_resources_override_parent_font() {
    for resources in ["<< >>", "10 7 R"] {
        let parsed = parse_pdf_bytes("empty-resources.pdf", &pdf(resources)).unwrap();
        assert_eq!(parsed.text_elements.len(), 1);
        assert_eq!(parsed.text_elements[0].font_name.as_deref(), Some("F"));
    }
}

/// 寸法が直接指定されていてもnullのFont継承で深度超過を返す。
#[test]
fn public_api_null_font_inheritance_obeys_depth_limit() {
    let limits = ParseLimits {
        max_container_nesting: 3,
        ..Default::default()
    };
    let source = String::from_utf8(pdf("null")).unwrap().replace(
        "2 0 obj << /Type /Pages /Resources << /Font << /F 4 0 R >> >> >> endobj",
        "2 0 obj << /Resources null /Parent 5 0 R >> endobj\n5 0 obj << /Resources null /Parent 6 0 R >> endobj\n6 0 obj << /Resources null /Parent 7 0 R >> endobj\n7 0 obj << /Resources << /Font << /F 4 0 R >> >> >> endobj",
    );
    let result = parse_pdf_bytes_with_limits("null-depth.pdf", source.as_bytes(), &limits);
    assert!(
        matches!(
            result,
            Err(ParseError::ResourceLimit {
                resource: "page inheritance depth",
                limit: 3,
                actual: 4
            })
        ),
        "{result:?}"
    );
}

/// 空辞書にFontがない場合、祖先のToUnicodeで文字を置き換えない。
#[test]
fn public_api_empty_resources_reject_global_cmap() {
    let cmap = "1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <6587>\n<42> <0042>\nendbfchar";
    for (resources, expected) in [("null", "文B"), ("<< >>", "AB")] {
        let source = String::from_utf8(pdf_with_content(
            resources,
            "BT /F 10 Tf 10 10 Td <4142> Tj ET",
        ))
        .unwrap()
        .replace("/BaseFont /Courier", "/BaseFont /Courier /ToUnicode 11 0 R")
        .replace(
            "%%EOF",
            &format!(
                "11 0 obj << /Length {} >> stream\n{cmap}\nendstream endobj\n%%EOF",
                cmap.len()
            ),
        );
        let parsed = parse_pdf_bytes("empty-cmap.pdf", source.as_bytes()).unwrap();
        assert_eq!(parsed.text_elements[0].text, expected, "{resources}");
    }
}
