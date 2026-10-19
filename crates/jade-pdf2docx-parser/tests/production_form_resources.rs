//! Form に属する Resources の参照・型と有限なグラフ検査の回帰。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// Resources の最外値を指定した Form stream を作る。
fn form(resources: &str, content: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] {resources} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// 正式なページから Form と親フォントを参照する PDF object 列を作る。
fn objects(resources: &str) -> Vec<String> {
    let mut objects = pages(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream("/Fm Do");
    objects[5] = stream("");
    objects[6] = form(resources, "BT /F 12 Tf 10 20 Td (FORM) Tj ET");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects
}

/// 明示された Resources の参照切れ・世代・循環・辞書以外を黙って無視しない。
#[test]
fn invalid_form_resource_dictionary_is_rejected() {
    for value in ["99 0 R", "11 1 R", "11 0 R", "[0]", "123", "(fake)"] {
        let mut objects = objects(&format!("/Resources {value}"));
        objects.push("11 0 R".into());
        assert!(
            matches!(
                parse_pdf_bytes("form-resources.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{value}"
        );
    }
}

/// Form の Font と XObject map も辞書型と参照の整合性を必要とする。
#[test]
fn invalid_form_resource_maps_are_rejected() {
    for category in ["Font", "XObject"] {
        for value in ["99 0 R", "11 1 R", "11 0 R", "[0]", "123"] {
            let mut objects = objects(&format!("/Resources << /{category} {value} >>"));
            objects.push("11 0 R".into());
            assert!(
                matches!(
                    parse_pdf_bytes("form-map.pdf", &pdf(&objects)),
                    Err(ParseError::InvalidPdfStructure { .. })
                ),
                "{category}: {value}"
            );
        }
    }
}

/// 局所 resource の個別参照でも欠落・別世代・alias 循環を拒否する。
#[test]
fn invalid_form_individual_references_are_rejected() {
    for category in ["Font", "XObject"] {
        for value in ["99 0 R", "11 1 R", "11 0 R"] {
            let mut objects = objects(&format!("/Resources << /{category} << /Item {value} >> >>"));
            objects.push("11 0 R".into());
            assert!(
                matches!(
                    parse_pdf_bytes("form-item.pdf", &pdf(&objects)),
                    Err(ParseError::InvalidPdfStructure { .. })
                ),
                "{category}: {value}"
            );
        }
    }
}

/// Form 内 Font の Type/Subtype と XObject の stream 型も検査する。
#[test]
fn malformed_form_resource_types_are_rejected() {
    for (category, value) in [
        ("Font", "<< >>"),
        ("Font", "<< /Type /Page /Subtype /Type1 >>"),
        ("Font", "<< /Type /Font /Subtype 1 >>"),
        (
            "Font",
            "<< /Type /Font /Subtype /Type1 /Length 0 >>\nstream\n\nendstream",
        ),
        ("XObject", "<< /Type /XObject /Subtype /Form >>"),
        (
            "XObject",
            "<< /Type /Page /Subtype /Form /Length 0 >>\nstream\n\nendstream",
        ),
        (
            "XObject",
            "<< /Type /XObject /Subtype 1 /Length 0 >>\nstream\n\nendstream",
        ),
    ] {
        let mut objects = objects(&format!("/Resources << /{category} << /Item 11 0 R >> >>"));
        objects.push(value.into());
        assert!(
            matches!(
                parse_pdf_bytes("form-type.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{category}: {value}"
        );
    }
}

/// 未描画の入れ子 Form にも局所 resource の構造検証を適用する。
#[test]
fn nested_unpainted_form_resources_are_validated() {
    let mut objects = objects("/Resources << /XObject << /Nested 11 0 R >> >>");
    objects[4] = stream("");
    objects.push(form("/Resources << /Font << /F 99 0 R >> >>", ""));
    assert!(matches!(
        parse_pdf_bytes("nested-form-resource.pdf", &pdf(&objects)),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// Resources 自体と Font map/個別 Font の正常な多段参照は本文を保持する。
#[test]
fn chained_form_resources_keep_the_local_font() {
    let mut objects = objects("/Resources 11 0 R");
    objects.extend([
        "12 0 R".into(),
        "<< /Font 13 0 R >>".into(),
        "<< /F 14 0 R >>".into(),
        "15 0 R".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ]);
    let parsed = parse_pdf_bytes("form-resource-alias.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Courier")
    );
}

/// Resources 省略と null は従来の親辞書利用を保持し、空辞書とは区別する。
#[test]
fn absent_and_null_resources_keep_inherited_font() {
    for resources in ["", "/Resources null", "/Resources 11 0 R"] {
        let mut objects = objects(resources);
        objects.push("null".into());
        let parsed = parse_pdf_bytes("form-resource-null.pdf", &pdf(&objects)).unwrap();
        assert_eq!(parsed.text_elements.len(), 1, "{resources}");
        assert_eq!(
            parsed.text_elements[0].font_name.as_deref(),
            Some("Helvetica"),
            "{resources}"
        );
    }
    let parsed = parse_pdf_bytes(
        "form-resource-empty.pdf",
        &pdf(&objects("/Resources << >>")),
    )
    .unwrap();
    assert_eq!(parsed.text_elements[0].font_name.as_deref(), Some("F"));
}

/// Form 同士の resource 共有・循環は alias 循環と区別し、有限回で検査する。
#[test]
fn shared_and_cyclic_form_graph_is_accepted() {
    let mut objects =
        objects("/Resources << /Font << /F 10 0 R >> /XObject << /Self 7 0 R /Other 11 0 R >> >>");
    objects.push(form(
        "/Resources << /Font << /F 10 0 R >> /XObject << /Back 7 0 R >> >>",
        "",
    ));
    let parsed = parse_pdf_bytes("form-resource-cycle.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
}

/// 共有 Form の最短到達深度は XObject 辞書の記載順に依存しない。
#[test]
fn shared_form_depth_is_independent_of_resource_order() {
    let limits = ParseLimits {
        max_container_nesting: 3,
        ..Default::default()
    };
    for entries in ["/Short 11 0 R /Long 12 0 R", "/Long 12 0 R /Short 11 0 R"] {
        let mut objects = objects(&format!("/Resources << /XObject << {entries} >> >>"));
        objects[4] = stream("");
        objects.push(form(
            "/Resources << /XObject << /A 14 0 R /B 15 0 R /C 16 0 R >> >>",
            "",
        ));
        for next in 13..=16 {
            objects.push(form(
                &format!("/Resources << /XObject << /Next {next} 0 R >> >>"),
                "",
            ));
        }
        objects.push(form("/Resources << >>", ""));
        assert!(
            parse_pdf_bytes_with_limits("form-resource-order.pdf", &pdf(&objects), &limits).is_ok(),
            "{entries}"
        );
    }
}

/// 線形な入れ子 Form resource graph を描画しない PDF を作る。
fn chain(depth: usize) -> Vec<String> {
    let mut objects = objects("/Resources << /XObject << /Next 11 0 R >> >>");
    objects[4] = stream("");
    for index in 1..depth {
        let resources = if index + 1 == depth {
            "/Resources << >>".into()
        } else {
            format!("/Resources << /XObject << /Next {} 0 R >> >>", 11 + index)
        };
        objects.push(form(&resources, ""));
    }
    objects
}

/// 未描画 Form のグラフ走査にも inclusive な入れ子上限を適用する。
#[test]
fn form_resource_graph_depth_is_bounded() {
    let bytes = pdf(&chain(8));
    let mut limits = ParseLimits {
        max_container_nesting: 7,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("form-resource-depth.pdf", &bytes, &limits),
        Err(ParseError::ResourceLimit {
            resource: "Form resource depth",
            actual: 8,
            limit: 7
        })
    ));
    limits.max_container_nesting = 8;
    assert!(parse_pdf_bytes_with_limits("form-resource-depth.pdf", &bytes, &limits).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
    /// 任意の深さの Form 局所 Font の世代不一致を成功扱いしない。
    #[test]
    fn nested_wrong_generation_is_rejected(depth in 2usize..10, location in 0usize..10) {
        let mut objects = chain(depth);
        let index = location % depth;
        let object = if index == 0 { 6 } else { 9 + index };
        objects[object] = objects[object].replace("/Resources <<", "/Resources << /Font << /Bad 10 1 R >>");
        let result = parse_pdf_bytes("form-generation-property.pdf", &pdf(&objects));
        prop_assert!(matches!(result, Err(ParseError::InvalidPdfStructure { .. })), "depth={depth}, location={index}");
    }
}
