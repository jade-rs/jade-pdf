//! ExtGStateの透明度と文字描画モードを組み合わせた抽出回帰。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 親Resources、ページ本文、局所Resourcesを持つFormを正式PDFへ組み立てる。
fn objects(page: &str, form: &str, local: &str) -> Vec<String> {
    let resources = "/Font << /F 10 0 R >> /ExtGState << /Zero << /ca 0 /CA 0 >> /Fill << /ca 0 >> /Stroke << /CA 0 >> /Half << /ca 0.5 /CA 0.5 >> /Indirect 11 0 R >> /XObject << /Fm 7 0 R >>";
    let mut objects = pages(&format!(
        "/MediaBox [0 0 500 500] /Resources << {resources} >>"
    ));
    objects[4] = stream(page);
    objects[5] = stream("BT /F 10 Tf 20 20 Td (Next page) Tj ET");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] {local} /Length {} >>\nstream\n{form}\nendstream",
        form.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    objects.push("<< /ca 12 0 R /CA 0 >>".into());
    objects.push("0".into());
    objects
}

/// 正式PDFを公開parser入口で解析する。
fn parse(page: &str, form: &str, local: &str) -> ParsedDocument {
    parse_pdf_bytes("alpha.pdf", &pdf(&objects(page, form, local))).unwrap()
}

/// 完全透明な文字は抽出せず、文字送りとQ復元および別ページの既定値を保持する。
#[test]
fn transparent_draws_advance_without_entering_text() {
    for in_form in [false, true] {
        let content = "q /Zero gs BT /F 10 Tf 20 20 Td (Hidden) Tj /Half gs (Visible) Tj ET Q BT /F 10 Tf 20 40 Td (Restored) Tj ET";
        let parsed = parse(if in_form { "/Fm Do" } else { content }, content, "");
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .filter(|t| t.page == 0)
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            ["Visible", "Restored"]
        );
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .find(|t| t.text == "Visible")
                .unwrap()
                .bounds
                .x0,
            56.
        );
        assert!(
            parsed
                .text_elements
                .iter()
                .any(|t| t.page == 1 && t.text == "Next page")
        );
    }
}

/// 省略成分は継承し、escaped nameや不正なgs operandを正しく区別する。
#[test]
fn partial_updates_and_invalid_operands_keep_previous_alpha() {
    for in_form in [false, true] {
        let content = "/Zero gs /Fill gs /Missing gs /Half 1 gs [( /Half gs )] /Unknown BDC BT /F 10 Tf 20 20 Td (Hidden) Tj /H#61lf gs 0 20 Td (Visible) Tj ET";
        let parsed = parse(if in_form { "/Fm Do" } else { content }, content, "");
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .filter(|t| t.page == 0)
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            ["Visible"]
        );
    }
}

proptest! {
    /// 任意の短い状態更新列でも、成分ごとの更新とq/Q復元が独立した期待値に一致する。
    #[test]
    fn alpha_updates_preserve_paint_decisions(operations in prop::collection::vec(0_u8..6, 1..25), mode in 0_u8..8, in_form in any::<bool>()) {
        let mut fill = true;
        let mut stroke = true;
        let mut saved = Vec::new();
        let mut content = String::new();
        let mut expected = Vec::new();
        for (index, operation) in operations.iter().enumerate() {
            match operation {
                0 => { content.push_str("/Fill gs "); fill = false; }
                1 => { content.push_str("/Stroke gs "); stroke = false; }
                2 => { content.push_str("/Half gs "); fill = true; stroke = true; }
                3 => { content.push_str("q "); saved.push((fill, stroke)); }
                4 if !saved.is_empty() => { content.push_str("Q "); (fill, stroke) = saved.pop().unwrap(); }
                _ => { content.push_str("/Missing gs "); }
            }
            let label = format!("Probe{index}");
            content.push_str(&format!("BT /F 10 Tf {mode} Tr 20 {} Td ({label}) Tj ET ", 20 + index * 12));
            let paints = match mode { 0 | 4 => fill, 1 | 5 => stroke, 2 | 6 => fill || stroke, _ => false };
            if paints { expected.push(label); }
        }
        for _ in saved { content.push_str("Q "); }
        let parsed = parse(if in_form { "/Fm Do" } else { &content }, &content, "");
        let actual = parsed.text_elements.iter().filter(|text| text.page == 0).map(|text| text.text.clone()).collect::<Vec<_>>();
        prop_assert_eq!(actual, expected);
    }
}

/// Contents配列の境界を越えてalphaを維持し、完全透明なページを本文へ戻さない。
#[test]
fn contents_array_keeps_alpha_and_transparent_page_stays_empty() {
    let mut objects = objects("/Zero gs", "", "");
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 13 0 R] >>".into();
    objects[5] = stream("");
    objects.push(stream("BT /F 10 Tf 20 20 Td (Hidden) Tj ET"));
    let parsed = parse_pdf_bytes("transparent.pdf", &pdf(&objects)).unwrap();
    assert!(
        parsed.text_elements.is_empty(),
        "{:?}",
        parsed.text_elements
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        let native = parse_native("empty", &objects);
        assert!(
            native.text_elements.is_empty(),
            "{:?}",
            native.text_elements
        );
        assert!(
            native.text_preview.trim().is_empty(),
            "{}",
            native.text_preview
        );
    }
}

/// 他のnative回帰と衝突しない一時ファイルを解析して削除する。
#[cfg(not(target_arch = "wasm32"))]
fn parse_native(label: &str, objects: &[String]) -> ParsedDocument {
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-alpha-{label}-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, pdf(objects)).unwrap();
    let parsed = jade_pdf2docx_parser::parse_pdf(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    parsed
}

/// nativeの文字置換でも透明文字を再混入させず、可視文字だけを返す。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_extraction_does_not_restore_transparent_text() {
    let content =
        "q /Zero gs BT /F 10 Tf 20 20 Td (Hidden) Tj ET Q BT /F 10 Tf 20 20 Td (Visible) Tj ET";
    let parsed = parse_native("overlap", &objects(content, "", ""));
    assert!(
        !parsed.text_preview.contains("Hidden"),
        "{}",
        parsed.text_preview
    );
    let text = parsed
        .text_elements
        .iter()
        .filter(|t| t.page == 0)
        .map(|t| t.text.as_str())
        .collect::<String>();
    assert_eq!(text, "Visible");
    let parsed = parse_native(
        "shared-form",
        &objects(
            "q /Zero gs /Fm Do Q /Fm Do",
            "BT /F 10 Tf 20 20 Td (Visible) Tj ET",
            "",
        ),
    );
    let text = parsed
        .text_elements
        .iter()
        .filter(|t| t.page == 0)
        .map(|t| t.text.as_str())
        .collect::<String>();
    assert_eq!(text.trim(), "Visible");
}

/// nativeページとFormでもfill/strokeを区別し、半透明の本文を保持する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_alpha_modes_keep_visible_page_and_form_text() {
    let mut content = String::new();
    let mut expected = String::new();
    for (index, (name, fill, stroke)) in [
        ("Fill", false, true),
        ("Stroke", true, false),
        ("Zero", false, false),
        ("Half", true, true),
        ("Indirect", false, false),
    ]
    .into_iter()
    .enumerate()
    {
        for mode in 0..3 {
            let label = format!("Probe{name}{mode}");
            content.push_str(&format!(
                "q /{name} gs BT /F 10 Tf {mode} Tr 20 {} Td ({label}) Tj ET Q ",
                20 + (index * 3 + mode) * 25
            ));
            if match mode {
                0 => fill,
                1 => stroke,
                _ => fill || stroke,
            } {
                expected.push_str(&label);
            }
        }
    }
    for in_form in [false, true] {
        let parsed = parse_native(
            if in_form { "modes-form" } else { "modes-page" },
            &objects(if in_form { "/Fm Do" } else { &content }, &content, ""),
        );
        let actual = parsed
            .text_elements
            .iter()
            .filter(|t| t.page == 0)
            .map(|t| t.text.trim())
            .collect::<String>();
        assert_eq!(actual, expected, "form={in_form}");
    }
}

/// fill/strokeの独立したalphaをTrのpaint種別へ適用し、半透明は残す。
#[test]
fn rendering_modes_use_their_own_alpha() {
    for mode in 0..8 {
        for (name, fill, stroke) in [
            ("Fill", false, true),
            ("Stroke", true, false),
            ("Zero", false, false),
            ("Half", true, true),
            ("Indirect", false, false),
        ] {
            let content = format!("/{name} gs BT /F 10 Tf {mode} Tr 20 20 Td (Probe) Tj ET");
            let paints = match mode {
                0 | 4 => fill,
                1 | 5 => stroke,
                2 | 6 => fill || stroke,
                _ => false,
            };
            for in_form in [false, true] {
                let parsed = parse(if in_form { "/Fm Do" } else { &content }, &content, "");
                assert_eq!(
                    parsed.text_elements.iter().any(|t| t.text == "Probe"),
                    paints,
                    "mode={mode}, state={name}, form={in_form}"
                );
            }
        }
    }
}

/// 呼び出し元alphaを継承し、局所gsは同名の親定義を上書きする。
#[test]
fn form_inherits_alpha_and_resolves_local_graphics_state() {
    let page = "q /Zero gs /Fm Do Q BT /F 10 Tf 20 40 Td (Parent) Tj ET";
    let form = "BT /F 10 Tf 20 20 Td (Hidden) Tj /Zero gs 0 20 Td (Local) Tj ET";
    let local = "/Resources << /Font << /F 10 0 R >> /ExtGState << /Zero << /ca 1 /CA 1 >> >> >>";
    let parsed = parse(page, form, local);
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>(),
        ["Parent", "Next page", "Local"]
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        let parsed = parse_native("inherit", &objects(page, form, local));
        assert!(
            !parsed.text_preview.contains("Hidden"),
            "{}",
            parsed.text_preview
        );
        for text in ["Parent", "Local", "Next page"] {
            assert!(
                parsed.text_preview.contains(text),
                "{}",
                parsed.text_preview
            );
        }
    }
}
