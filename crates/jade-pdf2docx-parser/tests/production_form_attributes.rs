//! Form の必須 BBox と省略可能な Matrix/FormType の型・参照・境界を検査する。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 属性だけを切り替えられる Form stream を生成する。
fn form(attributes: &str) -> String {
    let content = "BT /F 12 Tf 10 20 Td (FORM) Tj ET";
    format!(
        "<< /Type /XObject /Subtype /Form {attributes} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// 正式なページの Resources から Form と本文フォントを参照する。
fn objects(attributes: &str) -> Vec<String> {
    let mut objects = pages(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream("/Fm Do");
    objects[5] = stream("");
    objects[6] = form(attributes);
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects
}

/// 属性不正を通常 bytes 入口でも型付き構造エラーとして要求する。
fn assert_invalid(objects: &[String], label: &str) {
    let result = parse_pdf_bytes("form-attributes.pdf", &pdf(objects));
    assert!(
        matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
        "{label}: {result:?}"
    );
}

/// BBox の省略・null・配列以外・座標数不正を成功扱いにしない。
#[test]
fn missing_and_malformed_bbox_are_rejected() {
    for attributes in [
        "",
        "/BBox null",
        "/BBox 1",
        "/BBox (0 0 100 100)",
        "/BBox << /Fake [0 0 100 100] >>",
        "/BBox []",
        "/BBox [0 0 100]",
        "/BBox [0 0 100 100 100]",
    ] {
        assert_invalid(&objects(attributes), attributes);
    }
}

/// PDF 数値以外と f32 で非有限になる値・寸法を拒否する。
#[test]
fn bbox_coordinates_require_finite_pdf_numbers() {
    for value in [
        "null",
        "true",
        "/100",
        "(100)",
        "[100]",
        "<< /N 100 >>",
        "NaN",
        "inf",
        "1e2",
        "4000000000000000000000000000000000000000",
    ] {
        assert_invalid(&objects(&format!("/BBox [0 0 {value} 100]")), value);
    }
    assert_invalid(
        &objects(
            "/BBox [-300000000000000000000000000000000000000 0 300000000000000000000000000000000000000 100]",
        ),
        "finite coordinates with overflowing extent",
    );
}

/// BBox 全体と各座標の正常な alias を解決して本文を保持する。
#[test]
fn indirect_bbox_and_coordinate_aliases_are_accepted() {
    let mut objects = objects("/BBox 11 0 R");
    objects.extend([
        "12 0 R".into(),
        "[13 0 R 0 100 100]".into(),
        "14 0 R".into(),
        "-20".into(),
    ]);
    let parsed = parse_pdf_bytes("bbox-alias.pdf", &pdf(&objects)).unwrap();
    assert!(parsed.text_preview.contains("FORM"));
}

/// BBox と座標の欠落・世代不一致・循環参照を拒否する。
#[test]
fn broken_bbox_references_are_rejected() {
    for value in ["99 0 R", "11 1 R", "11 0 R"] {
        for attributes in [format!("/BBox {value}"), format!("/BBox [0 0 {value} 100]")] {
            let mut objects = objects(&attributes);
            objects.push("11 0 R".into());
            assert_invalid(&objects, &attributes);
        }
    }
}

/// 負座標・逆向きの対角・空矩形は型不正とせず、Form の品質警告を保持する。
#[test]
fn empty_and_reversed_bbox_do_not_hide_quality_warnings() {
    for bbox in [
        "[-20 -30 100 100]",
        "[100 100 0 0]",
        "[0 0 0 100]",
        "[0 0 0 0]",
    ] {
        let parsed =
            parse_pdf_bytes("bbox-control.pdf", &pdf(&objects(&format!("/BBox {bbox}")))).unwrap();
        assert!(
            parsed.quality.issues.iter().any(|issue| matches!(
                issue,
                jade_pdf2docx_parser::QualityIssue::UnsupportedXObject {
                    object_number: 7,
                    ..
                }
            )),
            "{bbox}"
        );
    }
}

/// 偽の BBox を入れ子・文字列・コメントから採用しない。
#[test]
fn fake_bbox_attributes_are_not_used() {
    for fake in [
        "/Note << /BBox [0 0 100 100] >>",
        "/Note (/BBox [0 0 100 100])",
        "% /BBox [0 0 100 100]\n",
    ] {
        assert_invalid(&objects(fake), fake);
    }
    let parsed = parse_pdf_bytes(
        "bbox-real.pdf",
        &pdf(&objects("/Note << /BBox 99 0 R >> /BBox [0 0 100 100]")),
    )
    .unwrap();
    assert!(parsed.text_preview.contains("FORM"));
}

/// 未描画の入れ子 Form でも必須描画属性を検査する。
#[test]
fn nested_unpainted_form_attributes_are_validated() {
    for attributes in [
        "",
        "/BBox [0 0 100 100] /Matrix true",
        "/BBox [0 0 100 100] /FormType 2",
    ] {
        let mut objects =
            objects("/BBox [0 0 100 100] /Resources << /XObject << /Inner 11 0 R >> >>");
        objects[4] = stream("");
        objects.push(form(attributes));
        assert_invalid(&objects, attributes);
    }
}

/// 省略/null Matrix は単位行列として本文を失わない。
#[test]
fn absent_and_null_matrix_use_identity() {
    for matrix in ["", "/Matrix null", "/Matrix 11 0 R"] {
        let mut objects = objects(&format!("/BBox [0 0 100 100] {matrix}"));
        objects.push("null".into());
        let parsed = parse_pdf_bytes("matrix-default.pdf", &pdf(&objects)).unwrap();
        let text = parsed
            .text_elements
            .iter()
            .find(|text| text.text == "FORM")
            .expect(matrix);
        assert_eq!((text.bounds.x0, text.bounds.y0), (10.0, 20.0), "{matrix}");
    }
}

/// Matrix の配列/各数値 alias を合成 CTM に適用する。
#[test]
fn indirect_matrix_coordinates_transform_text() {
    let mut objects = objects("/BBox [0 0 100 100] /Matrix 11 0 R");
    // 合成後の本文をページ内に置き、Matrix の参照解決を検証する。
    objects[1] = objects[1].replace("/MediaBox [0 0 100 100]", "/MediaBox [0 0 500 500]");
    objects[4] = stream("q 2 0 0 2 5 6 cm /Fm Do Q");
    objects.extend([
        "12 0 R".into(),
        "[13 0 R 0 0 14 0 R 3 4]".into(),
        "2".into(),
        "3".into(),
    ]);
    let parsed = parse_pdf_bytes("matrix-alias.pdf", &pdf(&objects)).unwrap();
    let text = parsed
        .text_elements
        .iter()
        .find(|text| text.text == "FORM")
        .expect("Form text");
    assert_eq!(
        (text.bounds.x0, text.bounds.y0, text.font_size),
        (51.0, 134.0, 72.0)
    );
}

/// 不正 Matrix を黙って読み飛ばさず通常解析を拒否する。
#[test]
fn malformed_and_broken_matrix_are_rejected() {
    for value in [
        "1",
        "[]",
        "[1 0 0 1 0]",
        "[1 0 0 1 0 0 0]",
        "[1e0 0 0 1 0 0]",
        "[NaN 0 0 1 0 0]",
        "[1 0 0 1 (0) 0]",
        "99 0 R",
        "11 1 R",
        "11 0 R",
        "[1 0 0 1 99 0 R 0]",
        "[1 0 0 1 11 1 R 0]",
        "[1 0 0 1 11 0 R 0]",
        "[1 0 0 1 null 0]",
        "[1 0 0 1 true 0]",
        "[1 0 0 1 /0 0]",
        "[1 0 0 1 [0] 0]",
        "[1 0 0 1 << /N 0 >> 0]",
        "[1 0 0 1 4000000000000000000000000000000000000000 0]",
    ] {
        let mut objects = objects(&format!("/BBox [0 0 100 100] /Matrix {value}"));
        objects.push("11 0 R".into());
        assert_invalid(&objects, value);
    }
}

/// FormType は省略/null の既定値または参照解決済み整数1を受け付ける。
#[test]
fn default_and_indirect_form_type_are_accepted() {
    for form_type in [
        "",
        "/FormType null",
        "/FormType 1",
        "/FormType +01",
        "/FormType 11 0 R",
    ] {
        let mut objects = objects(&format!("/BBox [0 0 100 100] {form_type}"));
        objects.extend(["12 0 R".into(), "1".into()]);
        assert!(
            parse_pdf_bytes("form-type.pdf", &pdf(&objects)).is_ok(),
            "{form_type}"
        );
    }
}

/// 未定義の FormType と実数/名前/配列/不正参照を許可しない。
#[test]
fn unsupported_or_malformed_form_type_is_rejected() {
    for value in [
        "0", "2", "-1", "1.0", "/1", "(1)", "[1]", "true", "99 0 R", "11 1 R", "11 0 R",
    ] {
        let mut objects = objects(&format!("/BBox [0 0 100 100] /FormType {value}"));
        objects.push("11 0 R".into());
        assert_invalid(&objects, value);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, failure_persistence: None, ..ProptestConfig::default() })]
    /// 属性 alias の深度境界を inclusive に保ち、超過は品質 fallback に変えない。
    #[test]
    fn attribute_reference_depth_is_inclusive(depth in 5usize..10, kind in 0usize..5) {
        let (attributes, terminal) = match kind {
            0 => ("/BBox 11 0 R", "[0 0 100 100]"),
            1 => ("/BBox [0 0 11 0 R 100]", "100"),
            2 => ("/BBox [0 0 100 100] /Matrix 11 0 R", "[1 0 0 1 0 0]"),
            3 => ("/BBox [0 0 100 100] /Matrix [11 0 R 0 0 1 0 0]", "1"),
            _ => ("/BBox [0 0 100 100] /FormType 11 0 R", "1"),
        };
        let mut objects = objects(attributes);
        for index in 0..depth {
            objects.push(if index + 1 == depth {
                terminal.into()
            } else {
                format!("{} 0 R", 12 + index)
            });
        }
        let bytes = pdf(&objects);
        let limits = ParseLimits { max_container_nesting: depth, ..Default::default() };
        prop_assert!(parse_pdf_bytes_with_limits("bbox-depth.pdf", &bytes, &limits).is_ok());
        let limits = ParseLimits { max_container_nesting: depth - 1, ..Default::default() };
        let result = parse_pdf_bytes_with_limits("bbox-depth.pdf", &bytes, &limits);
        prop_assert!(matches!(result, Err(ParseError::ResourceLimit { resource: "page attribute depth", .. })), "{:?}", result);
    }
}
