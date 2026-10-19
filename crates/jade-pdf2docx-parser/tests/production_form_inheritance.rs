//! Form呼び出し時の選択Fontと文字状態の継承を公開APIで検証する。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 親と異なる局所Fontを同じ名前で持つFormを作る。
fn form(content: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [-100 -100 500 500] /Resources << /Font << /F 11 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// 選択済みCourierと局所Helveticaを持つ正式なPDFのobject列を作る。
fn objects(parent: &str, child: &str) -> Vec<String> {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(parent);
    objects[5] = stream("");
    objects[6] = form(child);
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects
}

/// 局所Resourcesは選択済みFontを置換せず、局所Tfと復帰後の親を区別する。
#[test]
fn inherited_font_identity_and_size_survive_resource_shadowing() {
    let parsed = parse_pdf_bytes(
        "inherit.pdf",
        &pdf(&objects(
            "BT /F 24 Tf ET /Fm Do BT 10 100 Td (PARENT) Tj ET",
            "BT 10 20 Td (AA) Tj /F 9 Tf 0 30 Td (BB) Tj ET",
        )),
    )
    .unwrap();
    for (text, font, size) in [
        ("AA", "Courier", 24.),
        ("BB", "Helvetica", 9.),
        ("PARENT", "Courier", 24.),
    ] {
        let element = parsed
            .text_elements
            .iter()
            .find(|t| t.text == text)
            .unwrap();
        assert_eq!(element.font_name.as_deref(), Some(font), "{text}");
        assert_eq!(element.font_size, size, "{text}");
        if text == "AA" {
            assert!((element.bounds.x1 - element.bounds.x0 - 28.8).abs() < 0.001);
        }
    }
}

/// ToUnicodeとWidthsも選択時のFont定義から引き継ぐ。
#[test]
fn inherited_tounicode_and_widths_use_the_selected_font() {
    let mut objects = objects(
        "BT /F 24 Tf ET /Fm Do",
        "BT 10 20 Td <41> Tj /F 9 Tf 0 30 Td <41> Tj ET",
    );
    objects[9] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /LastChar 65 /Widths [500] /ToUnicode 12 0 R >>".into();
    objects[10] = "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 13 0 R >>".into();
    objects.push(stream("1 beginbfchar\n<41> <3042>\nendbfchar"));
    objects.push(stream("1 beginbfchar\n<41> <0058>\nendbfchar"));
    let parsed = parse_pdf_bytes("unicode.pdf", &pdf(&objects)).unwrap();
    let inherited = parsed
        .text_elements
        .iter()
        .find(|t| t.text == "あ")
        .unwrap();
    assert_eq!(inherited.font_name.as_deref(), Some("Helvetica"));
    assert_eq!(inherited.font_size, 24.);
    assert!((inherited.bounds.x1 - inherited.bounds.x0 - 12.).abs() < 0.001);
    let local = parsed.text_elements.iter().find(|t| t.text == "X").unwrap();
    assert_eq!(local.font_name.as_deref(), Some("Courier"));
    assert_eq!(local.font_size, 9.);
}

/// 文字間隔・単語間隔・横倍率・rise・leading・塗りもForm開始時に継承する。
#[test]
fn inherited_text_parameters_are_applied_to_form_draws() {
    let parsed = parse_pdf_bytes(
        "parameters.pdf",
        &pdf(&objects(
            "BT /F 20 Tf 2 Tc 3 Tw 150 Tz 4 Ts 15 TL ET 1 0 0 rg /Fm Do",
            "BT 10 40 Td (A A) Tj T* (NEXT) Tj ET",
        )),
    )
    .unwrap();
    let text = parsed
        .text_elements
        .iter()
        .find(|t| t.text == "A A")
        .unwrap();
    assert_eq!(text.fill_color.as_deref(), Some("FF0000"));
    assert_eq!(
        (text.bounds.x0, text.bounds.y0, text.font_size),
        (10., 44., 20.)
    );
    assert!((text.bounds.x1 - text.bounds.x0 - 64.5).abs() < 0.001);
    let next = parsed
        .text_elements
        .iter()
        .find(|t| t.text == "NEXT")
        .unwrap();
    assert_eq!(next.bounds.y0, 29.);
}

/// Form内のq/Qは局所Tfを巻き戻し、呼び出し元の選択Fontへ復帰する。
#[test]
fn form_graphics_restore_recovers_the_inherited_font_binding() {
    let parsed = parse_pdf_bytes(
        "restore.pdf",
        &pdf(&objects(
            "BT /F 24 Tf ET /Fm Do",
            "BT 10 20 Td q /F 9 Tf (LOCAL) Tj Q 0 30 Td (RESTORED) Tj ET",
        )),
    )
    .unwrap();
    let restored = parsed
        .text_elements
        .iter()
        .find(|t| t.text == "RESTORED")
        .unwrap();
    assert_eq!(restored.font_name.as_deref(), Some("Courier"));
    assert_eq!(restored.font_size, 24.);
}

/// 孫Formでも祖父の選択Fontと親の局所Tfを区別し、Do復帰後へ漏らさない。
#[test]
fn nested_forms_keep_the_resource_dictionary_of_each_selected_font() {
    let mut objects = objects(
        "BT /F 24 Tf ET /Fm Do BT 10 200 Td (PAGE) Tj ET",
        "q BT /F 9 Tf ET /Inner Do Q /Inner Do BT 10 40 Td (OUTER) Tj ET",
    );
    objects[6] = objects[6].replace("/F 11 0 R >>", "/F 11 0 R >> /XObject << /Inner 12 0 R >>");
    objects.push(form("BT 10 100 Td (INNER) Tj ET").replace("/F 11 0 R", "/F 13 0 R"));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>".into());
    let parsed = parse_pdf_bytes("nested.pdf", &pdf(&objects)).unwrap();
    let inner = parsed
        .text_elements
        .iter()
        .filter(|t| t.text == "INNER")
        .map(|t| (t.font_name.as_deref().unwrap(), t.font_size))
        .collect::<Vec<_>>();
    assert_eq!(inner, [("Helvetica", 9.), ("Courier", 24.)]);
    for text in ["OUTER", "PAGE"] {
        let text = parsed
            .text_elements
            .iter()
            .find(|t| t.text == text)
            .unwrap();
        assert_eq!(text.font_name.as_deref(), Some("Courier"));
        assert_eq!(text.font_size, 24.);
    }
}

/// 親のq/Qと複数Contentsを跨いだ選択を、反復Form呼び出しごとに固定する。
#[test]
fn parent_restore_and_contents_array_preserve_call_time_state() {
    let mut objects = objects("", "BT 10 40 Td (BODY) Tj ET");
    objects[2] = objects[2].replace("/Contents 5 0 R", "/Contents [5 0 R 12 0 R]");
    objects[4] = stream("BT /F 24 Tf ET 1 0 0 rg q BT /F 18 Tf ET 0 0 1 rg /Fm Do");
    objects.push(stream("Q /Fm Do"));
    let parsed = parse_pdf_bytes("contents.pdf", &pdf(&objects)).unwrap();
    let texts = parsed
        .text_elements
        .iter()
        .map(|t| (t.font_size, t.fill_color.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(texts, [(18., Some("0000FF")), (24., Some("FF0000"))]);
}

/// 引用符とTDの副作用を継承し、container内の偽命令は文字状態を変えない。
#[test]
fn inherited_quote_and_td_side_effects_ignore_container_operators() {
    let parsed = parse_pdf_bytes("operators.pdf", &pdf(&objects(
        "BT /F 20 Tf 0 -15 TD 3 2 (BEFORE) \" ET [/F 90 Tf 8 TL] << /K (0 Tc) >> /Unknown op /Fm Do",
        "BT 10 40 Td (A A) Tj T* (NEXT) Tj ET",
    ))).unwrap();
    let text = parsed
        .text_elements
        .iter()
        .find(|t| t.text == "A A")
        .unwrap();
    assert_eq!((text.bounds.y0, text.font_size), (40., 20.));
    assert!((text.bounds.x1 - text.bounds.x0 - 43.).abs() < 0.001);
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .find(|t| t.text == "NEXT")
            .unwrap()
            .bounds
            .y0,
        25.
    );
}

/// 継承したSubtypeでTr=7の版別可視性を判定し、局所同名Fontに影響されない。
#[test]
fn inherited_type3_visibility_uses_the_selected_font_subtype() {
    for pdf20 in [false, true] {
        for type3 in [false, true] {
            let mut objects = objects("BT /F 24 Tf 7 Tr ET /Fm Do", "BT 10 40 Td (BODY) Tj ET");
            if pdf20 {
                objects[0] = objects[0].replace("/Pages", "/Version /2.0 /Pages");
            }
            objects[if type3 { 9 } else { 10 }] = "<< /Type /Font /Subtype /Type3 /BaseFont /Custom /FirstChar 66 /LastChar 89 /Widths [600] >>".into();
            let parsed = parse_pdf_bytes("subtype.pdf", &pdf(&objects)).unwrap();
            assert_eq!(
                parsed.text_elements.iter().any(|t| t.text == "BODY"),
                type3 && !pdf20
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// Font名のescape・サイズ・反復描画でも選択済みFontの幅を保つ。
    #[test]
    fn inherited_font_width_is_independent_of_local_alias(size in 1u32..60, local_size in 1u32..60, count in 1usize..8) {
        let child = format!("BT 10 40 Td ({}) Tj /F {local_size} Tf 0 100 Td (LOCAL) Tj ET", "A".repeat(count));
        let parent = format!("BT /F {size} Tf ET /Fm Do");
        let mut objects = objects(&parent, &child);
        objects[4] = objects[4].replace("/F ", "/#46 ");
        let parsed = parse_pdf_bytes("property.pdf", &pdf(&objects)).unwrap();
        let inherited = parsed.text_elements.iter().find(|t| t.text == "A".repeat(count)).unwrap();
        prop_assert_eq!(inherited.font_name.as_deref(), Some("Courier"));
        prop_assert_eq!(inherited.font_size, size as f32);
        prop_assert!((inherited.bounds.x1 - inherited.bounds.x0 - count as f32 * size as f32 * 0.6).abs() < 0.002);
    }
}
