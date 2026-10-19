//! 不可視文字を公開APIの本文へ混入させず、文字原点と描画状態を維持する。
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
use page_attribute_pdf::{pages, pdf, stream};
use proptest::prelude::*;

/// 二ページと局所Formを持ち、正式なxref付きの文字描画PDFを作る。
fn source(first: &str, second: &str, form: &str) -> Vec<u8> {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(first);
    objects[5] = stream(second);
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> >> /Length {} >>\nstream\n{form}\nendstream",
        form.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    pdf(&objects)
}

/// ページ本文を抽出順に集め、描画命令間の分割には依存しない。
fn text(parsed: &ParsedDocument, page: usize) -> String {
    parsed
        .text_elements
        .iter()
        .filter(|element| element.page == page)
        .map(|element| element.text.as_str())
        .collect()
}

/// 3と7はpaintを行わず、他の有効モードでは文字が残る。
#[test]
fn invisible_modes_are_excluded_from_elements_and_preview() {
    for mode in 0..8 {
        let bytes = source(
            &format!("BT /F 10 Tf 10 50 Td {mode} Tr (MARK) Tj ET"),
            "",
            "",
        );
        let parsed = parse_pdf_bytes("mode.pdf", &bytes).unwrap();
        let expected = if matches!(mode, 3 | 7) { "" } else { "MARK" };
        assert_eq!(text(&parsed, 0), expected, "mode {mode}");
        assert_eq!(parsed.text_preview, expected, "preview mode {mode}");
    }
}

/// 不可視glyphのadvanceとTJ調整は後続の可視文字にも反映する。
#[test]
fn hidden_draws_advance_text_without_merging_across_the_gap() {
    let bytes = source(
        "BT /F 10 Tf 10 50 Td (A) Tj 3 Tr [(BC) -200] TJ 0 Tr (D) Tj ET",
        "",
        "",
    );
    let parsed = parse_pdf_bytes("advance.pdf", &bytes).unwrap();
    assert_eq!(text(&parsed, 0), "AD");
    assert_eq!(parsed.text_elements.len(), 2);
    assert!((parsed.text_elements[1].bounds.x0 - 30.).abs() < 0.01);
}

/// TrはBT/ETを越えて保持され、q/Qで復帰し、次ページでは初期化する。
#[test]
fn rendering_state_survives_text_objects_and_restores_graphics_state() {
    let bytes = source(
        "3 Tr BT /F 10 Tf 10 50 Td (SECRET) Tj ET q 0 Tr BT /F 10 Tf 10 70 Td (VISIBLE) Tj ET Q BT /F 10 Tf 10 90 Td (SECRET) Tj ET",
        "BT /F 10 Tf 10 50 Td (NEXT) Tj ET",
        "",
    );
    let parsed = parse_pdf_bytes("state.pdf", &bytes).unwrap();
    assert_eq!(text(&parsed, 0), "VISIBLE");
    assert_eq!(text(&parsed, 1), "NEXT");
    assert!(!parsed.text_preview.contains("SECRET"));
}

/// Formは呼び出し時のTrを継承し、Form内の変更を呼び出し元へ漏らさない。
#[test]
fn forms_inherit_mode_at_each_call_and_restore_the_parent() {
    let bytes = source(
        "3 Tr /Fm Do q 0 Tr /Fm Do Q BT /F 10 Tf 10 90 Td (SECRET) Tj ET",
        "",
        "BT /F 10 Tf 10 50 Td (INHERITED) Tj ET 0 Tr BT /F 10 Tf 10 70 Td (LOCAL) Tj ET",
    );
    let parsed = parse_pdf_bytes("form.pdf", &bytes).unwrap();
    assert_eq!(text(&parsed, 0), "LOCALINHERITEDLOCAL");
    assert!(!parsed.text_preview.contains("SECRET"));
}

/// Contents配列の境界で状態を失わず、container内の偽Trを実行しない。
#[test]
fn contents_arrays_preserve_mode_and_ignore_fake_or_invalid_operands() {
    let mut objects = pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> >>");
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 6 0 R] >>".into();
    objects[3] = "<< /Type /Page /Parent 2 0 R >>".into();
    objects[4] = stream("3 Tr");
    objects[5] = stream(
        "[0 Tr] << /Fake (0 Tr) >> (0 Tr) % 0 Tr\n 8 Tr -1 Tr 0.5 Tr BT /F 10 Tf 10 50 Td (SECRET) Tj 0 Tr (PUBLIC) Tj ET",
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    let parsed = parse_pdf_bytes("array.pdf", &pdf(&objects)).unwrap();
    assert_eq!(text(&parsed, 0), "PUBLIC");
}

/// 入れ子Formも直前の状態を継承し、子のTr変更を親へ漏らさない。
#[test]
fn nested_forms_inherit_and_restore_modes() {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream("3 Tr /Fm Do");
    objects[5] = stream("");
    let parent = "/Child Do BT /F 10 Tf 10 90 Td (SECRET) Tj ET q 0 Tr /Child Do Q";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Child 11 0 R >> >> /Length {} >>\nstream\n{parent}\nendstream",
        parent.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    let child = "BT /F 10 Tf 10 50 Td (INHERITED) Tj ET 0 Tr BT /F 10 Tf 10 70 Td (LOCAL) Tj ET";
    objects.push(format!("<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] /Length {} >>\nstream\n{child}\nendstream", child.len()));
    let parsed = parse_pdf_bytes("nested.pdf", &pdf(&objects)).unwrap();
    assert_eq!(text(&parsed, 0), "LOCALINHERITEDLOCAL");
}

/// nativeが不可視文字しか返さない場合も本文を空に保つ。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_all_invisible_text_remains_empty() {
    let bytes = source(
        "BT /F 10 Tf 10 50 Td 3 Tr (SECRET) Tj ET",
        "BT /F 10 Tf 10 50 Td 7 Tr (HIDDEN) Tj ET",
        "",
    );
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-all-invisible-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    let result = jade_pdf2docx_parser::parse_pdf(&path);
    std::fs::remove_file(&path).unwrap();
    let parsed = result.unwrap();
    assert!(parsed.text_elements.is_empty());
    assert!(parsed.text_preview.is_empty());
}

/// 不可視文字と混在してもToUnicodeの補助平面文字・内部空白を保持する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_filter_preserves_supplementary_unicode_and_spaces() {
    let mut objects = pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> >>");
    objects[4] =
        stream("BT /F 10 Tf 10 50 Td 3 Tr (SECRET) Tj ET BT /F 10 Tf 10 80 Td 0 Tr (A B) Tj ET");
    objects[5] = stream("");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 11 0 R >>".into());
    objects.push(stream(
        "1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <41> <D83DDE00> endbfchar",
    ));
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-visible-unicode-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, pdf(&objects)).unwrap();
    let result = jade_pdf2docx_parser::parse_pdf(&path);
    std::fs::remove_file(&path).unwrap();
    let parsed = result.unwrap();
    assert_eq!(text(&parsed, 0), "😀 B");
    assert_eq!(parsed.text_preview, "😀 B");
}

/// pure側と文字数が異なるActualTextでもnative結果の空白を保持する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_filter_preserves_spaces_when_actual_text_changes_character_count() {
    let bytes = source(
        "BT /F 10 Tf 10 50 Td 3 Tr (SECRET) Tj ET BT /F 10 Tf 10 80 Td 0 Tr /Span << /ActualText (V W) >> BDC (A) Tj EMC ET",
        "",
        "",
    );
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-visible-actual-text-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    let result = jade_pdf2docx_parser::parse_pdf(&path);
    std::fs::remove_file(&path).unwrap();
    let parsed = result.unwrap();
    assert_eq!(text(&parsed, 0), "V W");
    assert_eq!(parsed.text_preview, "V W");
}

/// native抽出でも重なる不可視文字を本文へ復活させない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_extraction_filters_overlapping_invisible_characters() {
    let bytes = source(
        "BT /F 10 Tf 10 50 Td 3 Tr (SECRET) Tj ET BT /F 10 Tf 10 50 Td 0 Tr (PUBLIC) Tj ET BT /F 10 Tf 10 80 Td 7 Tr (HIDDEN) Tj ET",
        "",
        "",
    );
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-invisible-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    let result = jade_pdf2docx_parser::parse_pdf(&path);
    std::fs::remove_file(&path).unwrap();
    let parsed = result.unwrap();
    assert_eq!(text(&parsed, 0), "PUBLIC");
    assert_eq!(parsed.text_preview, "PUBLIC");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意のモード列でも抽出文字はpaintを行う命令だけに一致する。
    #[test]
    fn only_painted_text_survives(modes in prop::collection::vec(0u8..8, 1..30)) {
        let mut content = String::new();
        let mut expected = String::new();
        for (index, mode) in modes.into_iter().enumerate() {
            let ch = char::from(b'A' + (index % 26) as u8);
            content.push_str(&format!("{mode} Tr BT /F 10 Tf 10 {} Td ({ch}) Tj ET\n", 20 + index * 12));
            if !matches!(mode, 3 | 7) { expected.push(ch); }
        }
        let parsed = parse_pdf_bytes("property.pdf", &source(&content, "", "")).unwrap();
        prop_assert_eq!(text(&parsed, 0), expected);
    }
}
