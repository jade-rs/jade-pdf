//! Form 内本文・局所 Resources・描画状態と有限な処理量の回帰。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 局所フォントと変換行列を持つ Form の本文を作る。
fn form(content: &str, matrix: &str, resources: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [-100 -100 500 500] /Matrix [{matrix}] /Resources {resources} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// 親と Form で同じ Font 名が異なる定義を持つ正式 PDF を作る。
fn objects(content: &str) -> Vec<String> {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(content);
    objects[5] = stream("");
    objects[6] = form(
        "BT /F 12 Tf 10 20 Td (FORM TEXT) Tj ET",
        "1 0 0 1 3 4",
        "<< /Font << /F 11 0 R >> >>",
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    objects
}

/// 呼び出し CTM と Form Matrix を合成し、同名の局所 Font を優先する。
#[test]
fn local_font_and_form_matrix_are_used() {
    let parsed = parse_pdf_bytes("form.pdf", &pdf(&objects("q 2 0 0 2 5 6 cm /Fm Do Q"))).unwrap();
    let text = parsed
        .text_elements
        .iter()
        .find(|t| t.text == "FORM TEXT")
        .expect("Form text");
    assert_eq!(text.font_name.as_deref(), Some("Courier"));
    assert_eq!(
        (text.bounds.x0, text.bounds.y0, text.font_size),
        (31.0, 54.0, 24.0)
    );
    assert!(parsed.text_preview.contains("FORM TEXT"));
    assert!(
        !parsed.quality.issues.is_empty(),
        "clip/images remain unverified"
    );
}

/// q/Q 復帰と複数 cm の合成を描画ごとに保持する。
#[test]
fn repeated_calls_restore_and_compose_graphics_state() {
    let parsed = parse_pdf_bytes(
        "form.pdf",
        &pdf(&objects("q 2 0 0 2 0 0 cm 1 0 0 1 5 6 cm /Fm Do Q /Fm Do")),
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| (t.bounds.x0, t.bounds.y0))
            .collect::<Vec<_>>(),
        [(36.0, 60.0), (13.0, 24.0)]
    );
}

/// Form 内の text object も q/Q と cm の合成を反映する。
#[test]
fn form_text_restores_internal_graphics_state() {
    let mut objects = objects("q 2 0 0 2 5 6 cm /Fm Do Q");
    objects[6] = form(
        "q 2 0 0 2 0 0 cm BT /F 12 Tf 10 20 Td (FIRST) Tj ET Q BT /F 12 Tf 10 20 Td (SECOND) Tj ET",
        "1 0 0 1 3 4",
        "<< /Font << /F 11 0 R >> >>",
    );
    let parsed = parse_pdf_bytes("internal.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| (t.bounds.x0, t.bounds.y0, t.font_size))
            .collect::<Vec<_>>(),
        [(51.0, 94.0, 48.0), (31.0, 54.0, 24.0)]
    );
}

/// Form の graphics state 保存 stack も上限を持ち、巨大な q 列を保持しない。
#[test]
fn graphics_state_stack_is_bounded() {
    let mut objects = objects("/Fm Do");
    objects[6] = form(
        &format!("{}BT /F 12 Tf (TEXT) Tj ET", "q ".repeat(32)),
        "1 0 0 1 0 0",
        "<< /Font << /F 11 0 R >> >>",
    );
    assert!(matches!(
        parse_pdf_bytes_with_limits(
            "state.pdf",
            &pdf(&objects),
            &ParseLimits {
                max_container_nesting: 8,
                ..Default::default()
            }
        ),
        Err(ParseError::ResourceLimit {
            resource: "Form graphics state depth",
            ..
        })
    ));
}

/// 非 UTF-8 の先行文字列があっても後続 BT の元 byte 位置を取り違えない。
#[test]
fn non_utf8_literal_does_not_hide_later_text_objects() {
    let mut objects = objects("/Fm Do");
    objects[6] = form(
        "BT /F 12 Tf (FIRST ~) Tj ET BT /F 12 Tf 10 20 Td (SECOND) Tj ET",
        "1 0 0 1 0 0",
        "<< /Font << /F 11 0 R >> >>",
    );
    let mut bytes = pdf(&objects);
    let position = bytes.iter().position(|&b| b == b'~').unwrap();
    bytes[position] = 0x80;
    let parsed = parse_pdf_bytes("literal.pdf", &bytes).unwrap();
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|text| text.text == "SECOND")
    );
}

/// 局所 ToUnicode は同名の親 Font の文字対応と混ざらない。
#[test]
fn local_tounicode_is_applied() {
    let mut objects = objects("/Fm Do");
    objects[6] = form(
        "BT /F 12 Tf 10 20 Td <41> Tj ET",
        "1 0 0 1 0 0",
        "<< /Font << /F 11 0 R >> >>",
    );
    objects[10] = "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 12 0 R >>".into();
    objects.push(stream("1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <3042>\nendbfchar\n"));
    let parsed = parse_pdf_bytes("unicode.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.text_elements[0].text, "あ");
}

/// Form の有限な呼び出し深度は inclusive な上限を使う。
#[test]
fn form_depth_limit_is_inclusive() {
    let mut objects = objects("/Fm Do");
    // 全 Form を親辞書から参照し、構造検査の深さではなく実際の呼び出し深さを測る。
    let names = (12..=20)
        .map(|number| format!("/N{number} {number} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    objects[1] = format!(
        "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 500 500] /Resources << /Font << /F 11 0 R >> /XObject << /Fm 7 0 R {names} >> >> >>"
    );
    for number in 12..=20 {
        let content = if number == 20 {
            "BT /F 12 Tf (DEEP) Tj ET".to_owned()
        } else {
            format!("/N{} Do", number + 1)
        };
        objects.push(form(&content, "1 0 0 1 0 0", "null"));
    }
    objects[6] = form("/N12 Do", "1 0 0 1 0 0", "null");
    let bytes = pdf(&objects);
    let limits = ParseLimits {
        max_container_nesting: 10,
        ..Default::default()
    };
    assert!(
        parse_pdf_bytes_with_limits("deep.pdf", &bytes, &limits)
            .unwrap()
            .text_preview
            .contains("DEEP")
    );
    assert!(matches!(
        parse_pdf_bytes_with_limits(
            "deep.pdf",
            &bytes,
            &ParseLimits {
                max_container_nesting: 9,
                ..limits
            }
        ),
        Err(ParseError::ResourceLimit {
            resource: "Form depth",
            limit: 9,
            actual: 10
        })
    ));
}

/// Contents 配列にまたがる operand と CTM も同じページで保持する。
#[test]
fn content_array_keeps_draw_state() {
    let mut objects = objects("q 2 0 0 2 5 6 cm /Fm");
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 12 0 R] >>".into();
    objects.push(stream("Do Q"));
    let parsed = parse_pdf_bytes("form.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].bounds.x0, 31.0);
}

/// 入れ子の同名 XObject と Font はそれぞれの局所辞書で解決する。
#[test]
fn nested_form_uses_its_own_resources() {
    let mut objects = objects("/Fm Do");
    objects[6] = form(
        "1 0 0 1 7 8 cm /Fm Do",
        "1 0 0 1 3 4",
        "<< /XObject << /Fm 12 0 R >> >>",
    );
    objects.push(form(
        "BT /F 10 Tf 1 2 Td (NESTED) Tj ET",
        "1 0 0 1 9 10",
        "<< /Font << /F 11 0 R >> >>",
    ));
    let parsed = parse_pdf_bytes("form.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    let text = &parsed.text_elements[0];
    assert_eq!(
        (
            text.text.as_str(),
            text.font_name.as_deref(),
            text.bounds.x0,
            text.bounds.y0
        ),
        ("NESTED", Some("Courier"), 20.0, 24.0)
    );
}

/// 自己参照は stack overflow や無限展開を起こさず、未検証診断を残す。
#[test]
fn recursive_form_stops_without_repeating_text() {
    let mut objects = objects("/Fm Do");
    objects[6] = form(
        "BT /F 12 Tf (ONCE) Tj ET /Fm Do",
        "1 0 0 1 0 0",
        "<< /Font << /F 11 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    let parsed = parse_pdf_bytes("cycle.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>(),
        ["ONCE"]
    );
    assert!(!parsed.quality.issues.is_empty());
}

/// 非圧縮 Form も描画回数に応じた累積 byte 予算を超えたら失敗する。
#[test]
fn repeated_uncompressed_form_is_bounded() {
    let bytes = pdf(&objects(&"/Fm Do ".repeat(100)));
    assert!(matches!(
        parse_pdf_bytes_with_limits(
            "limit.pdf",
            &bytes,
            &ParseLimits {
                max_total_decoded_bytes: 128,
                ..Default::default()
            }
        ),
        Err(ParseError::ResourceLimit {
            resource: "total decoded bytes",
            ..
        })
    ));
}

/// 文字列・container と inline binary 内の偽 Do から本文を復活させない。
#[test]
fn fake_do_and_inline_binary_do_not_extract_forms() {
    for content in [
        "(/Fm Do) Tj",
        "[/Fm Do] TJ",
        "% /Fm Do\n",
        "BI /W 2 /H 1 /BPC 8 /CS /RGB ID /Fm Do EI",
    ] {
        let parsed = parse_pdf_bytes("fake.pdf", &pdf(&objects(content))).unwrap();
        assert!(parsed.text_elements.is_empty(), "{content}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
    /// 任意の平行移動でも Form の局所位置・ページ所属・描画回数を保つ。
    #[test]
    fn translations_preserve_form_text(x in 0u16..100, y in 0u16..100, count in 1usize..8) {
        let content = format!("q 1 0 0 1 {x} {y} cm {} Q", "/Fm Do ".repeat(count));
        let parsed = parse_pdf_bytes("property.pdf", &pdf(&objects(&content))).unwrap();
        prop_assert_eq!(parsed.text_elements.len(), count);
        for text in parsed.text_elements {
            prop_assert_eq!((text.page, text.bounds.x0, text.bounds.y0), (0, f32::from(x) + 13.0, f32::from(y) + 24.0));
        }
    }
}
