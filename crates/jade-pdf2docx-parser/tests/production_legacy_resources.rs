//! RootなしPDFにもResourcesの型・世代・循環・局所Form検証を適用する。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;

/// 正確なcontent Lengthと正常な親Fontを持つ互換入力を作る。
fn pdf(resources: &str, extra: &str) -> Vec<u8> {
    let content = "BT /F 10 Tf 10 10 Td (AB) Tj ET";
    format!("%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 100 200] /Parent 2 0 R {resources} /Contents 3 0 R >> endobj\n2 0 obj << /Type /Pages /Resources << /Font << /F 4 0 R >> >> >> endobj\n3 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n4 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Courier >> endobj\n{extra}\n%%EOF\n", content.len()).into_bytes()
}

/// 部分本文を持つ成功ではなく、呼び出し元が判別できる構造エラーを要求する。
fn assert_invalid(resources: &str, extra: &str) {
    let result = parse_pdf_bytes("legacy-bad-resources.pdf", &pdf(resources, extra));
    assert!(
        matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
        "{resources}, {extra}: {result:?}"
    );
}

/// Resourcesの不正型を空辞書や親Fontとして成功扱いにしない。
#[test]
fn invalid_direct_resource_type_is_rejected() {
    for value in ["42", "/Name", "[]", "(text)", "true"] {
        assert_invalid(&format!("/Resources {value}"), "");
    }
}

/// 参照の欠落・世代不一致・循環・最終値の不正型を拒否する。
#[test]
fn invalid_indirect_resources_are_rejected() {
    for (value, extra) in [
        ("99 0 R", ""),
        ("8 0 R", "8 7 obj << >> endobj"),
        ("8 7 R", "8 7 obj 9 3 R endobj\n9 3 obj 8 7 R endobj"),
        ("8 7 R", "8 7 obj [] endobj"),
        ("8 7 R", "8 7 obj << >> junk endobj"),
    ] {
        assert_invalid(&format!("/Resources {value}"), extra);
    }
}

/// nullや欠落から選ぶ親Resourcesにも同じ不正型検査を適用する。
#[test]
fn inherited_invalid_resources_are_rejected() {
    for own in ["", "/Resources null", "/Resources 8 7 R"] {
        let mut bytes = String::from_utf8(pdf(own, "8 7 obj null endobj")).unwrap();
        bytes = bytes.replace("/Resources << /Font << /F 4 0 R >> >>", "/Resources [0]");
        assert!(matches!(
            parse_pdf_bytes("legacy-parent-resources.pdf", bytes.as_bytes()),
            Err(ParseError::InvalidPdfStructure { .. })
        ));
    }
}

/// 継承を必要とするParentの破損をResources欠落として隠さない。
#[test]
fn invalid_resource_parent_is_rejected() {
    for (parent, extra) in [
        ("99 0 R", ""),
        ("2 7 R", ""),
        ("[]", ""),
        ("8 7 R", "8 7 obj [] endobj"),
        ("8 7 R", "8 7 obj << /Parent 8 7 R >> endobj"),
    ] {
        let bytes = String::from_utf8(pdf("", extra))
            .unwrap()
            .replace("/Parent 2 0 R", &format!("/Parent {parent}"));
        assert!(
            matches!(
                parse_pdf_bytes("legacy-parent.pdf", bytes.as_bytes()),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{parent}"
        );
    }
}

/// Fontのmap・entry・Type・Subtypeの破損を本文の推定Fontで隠さない。
#[test]
fn invalid_local_fonts_are_rejected() {
    for resources in [
        "<< /Font [] >>",
        "<< /Font 99 0 R >>",
        "<< /Font << /F 4 7 R >> >>",
        "<< /Font << /F [] >> >>",
        "<< /Font << /F << /Type /Wrong /Subtype /Type1 >> >> >>",
        "<< /Font << /F << /Subtype /Type1 >> >> >>",
        "<< /Font << /F << /Type /Font >> >> >>",
        "<< /Font << /F << /Type /Font /Subtype 12 >> >> >>",
    ] {
        assert_invalid(&format!("/Resources {resources}"), "");
    }
}

/// XObjectのmap・世代・streamの型を実際の描画の有無によらず検査する。
#[test]
fn invalid_local_xobjects_are_rejected() {
    for (resources, extra) in [
        ("<< /XObject [] >>", ""),
        ("<< /XObject << /Fm 99 0 R >> >>", ""),
        ("<< /XObject << /Fm 4 0 R >> >>", ""),
        ("<< /XObject << /Fm << /Subtype /Form >> >> >>", ""),
        (
            "<< /XObject << /Fm 8 0 R >> >>",
            "8 7 obj << /Subtype /Image /Length 0 >> stream\n\nendstream endobj",
        ),
        (
            "<< /XObject << /Fm 8 7 R >> >>",
            "8 7 obj << /Type /Wrong /Subtype /Image /Length 0 >> stream\n\nendstream endobj",
        ),
    ] {
        assert_invalid(&format!("/Resources {resources}"), extra);
    }
}

/// Form局所Resourcesと幾何属性の破損も文書全体の構造エラーにする。
#[test]
fn invalid_form_resources_and_geometry_are_rejected() {
    for attributes in [
        "/BBox [0 0 100 100] /Resources []",
        "/BBox [0 0 100 100] /Resources << /Font << /F 4 7 R >> >>",
        "/BBox [0 0 100 100] /Matrix [1 0 0]",
        "/Resources << >>",
    ] {
        let extra = format!(
            "8 7 obj << /Type /XObject /Subtype /Form {attributes} /Length 0 >> stream\n\nendstream endobj"
        );
        assert_invalid("/Resources << /XObject << /Fm 8 7 R >> >>", &extra);
    }
}

/// 欠落・直接/間接nullは継承し、最寄り空辞書は親を上書きする。
#[test]
fn valid_resources_preserve_null_and_empty_semantics() {
    for (resources, font) in [
        ("", "Courier"),
        ("/Resources null", "Courier"),
        ("/Resources 8 7 R", "Courier"),
        ("/Resources << >>", "F"),
        ("/Resources 9 3 R", "F"),
        ("/Resources << /Font << /F null >> /XObject null >>", "F"),
    ] {
        let bytes = pdf(resources, "8 7 obj null endobj\n9 3 obj << >> endobj");
        let parsed = parse_pdf_bytes("legacy-good-resources.pdf", &bytes).unwrap();
        assert_eq!(parsed.text_elements[0].text, "AB");
        assert_eq!(parsed.text_elements[0].font_name.as_deref(), Some(font));
    }
}

/// Resourcesの間接chainはinclusive深度上限で成功し、一つ超えると拒否する。
#[test]
fn resource_reference_depth_is_inclusive() {
    let extra = "8 7 obj 9 3 R endobj\n9 3 obj 10 0 R endobj\n10 0 obj 11 0 R endobj\n11 0 obj << >> endobj";
    let bytes = pdf("/Resources 8 7 R", extra);
    let mut limits = ParseLimits {
        max_container_nesting: 3,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("legacy-depth.pdf", &bytes, &limits),
        Err(ParseError::ResourceLimit {
            resource: "page attribute depth",
            limit: 3,
            actual: 4
        })
    ));
    limits.max_container_nesting = 4;
    assert!(parse_pdf_bytes_with_limits("legacy-depth.pdf", &bytes, &limits).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 同番号更新後の最新世代だけを採用し、異なる参照世代を成功にしない。
    #[test]
    fn resource_generation_matches_latest_object(reference in 0u16..10, actual in 0u16..10) {
        let extra = format!("8 12 obj [] endobj\n8 {actual} obj << /Font << /F 4 0 R >> >> endobj");
        let result = parse_pdf_bytes("legacy-resource-model.pdf", &pdf(&format!("/Resources 8 {reference} R"), &extra));
        if reference == actual {
            let parsed = result.unwrap();
            prop_assert_eq!(parsed.text_elements[0].font_name.as_deref(), Some("Courier"));
        } else {
            prop_assert!(matches!(result, Err(ParseError::InvalidPdfStructure { .. })), "wrong-generation resource succeeded");
        }
    }
}
