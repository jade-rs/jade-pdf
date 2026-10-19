//! ページとFormを合わせたpreviewの文字数・重複・可視本文を公開APIで検査する。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;

#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 非BMP文字もUTF-16BEの表示operandとして保存する。
fn shown(text: &str) -> String {
    let mut hex = String::from("FEFF");
    for code in text.encode_utf16() {
        hex.push_str(&format!("{code:04X}"));
    }
    format!("BT /F 1 Tf 10 20 Td <{hex}> Tj ET")
}

/// 局所Fontを持ち、長い本文全体がBBoxに入るFormを作る。
fn form(content: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 10000 10000] /Resources << /Font << /F 11 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// 正式xrefと独立したページ・FormのFontを持つ入力を作る。
fn objects(page_content: &str, form_content: &str, calls: usize) -> Vec<String> {
    let mut objects = pages(
        "/MediaBox [0 0 10000 10000] /CropBox [0 0 10000 10000] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(&format!("{page_content}\n{}", "/Fm Do\n".repeat(calls)));
    objects[5] = stream("");
    objects[6] = form(form_content);
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    objects
}

/// ページ本文・改行・Form本文の総Unicode文字数を1,024文字へ制限する。
#[test]
fn combined_preview_limit_counts_unicode_and_separator() {
    let page = "あ".repeat(1000);
    let form = "𝄞".repeat(40);
    let parsed = parse_pdf_bytes(
        "combined-preview.pdf",
        &pdf(&objects(&shown(&page), &shown(&form), 1)),
    )
    .unwrap();
    assert_eq!(parsed.text_elements.len(), 2);
    assert_eq!(parsed.text_elements[1].text, form);
    assert_eq!(parsed.text_preview, format!("{page}\n{}", "𝄞".repeat(23)));
    assert_eq!(parsed.text_preview.chars().count(), 1024);
}

/// Formだけの文書にも同じ上限を適用し、本文そのものは切断しない。
#[test]
fn form_only_preview_is_bounded() {
    let text = "文".repeat(1100);
    let parsed = parse_pdf_bytes("form-preview.pdf", &pdf(&objects("", &shown(&text), 1))).unwrap();
    assert_eq!(parsed.text_elements[0].text, text);
    assert_eq!(parsed.text_preview, "文".repeat(1024));
}

/// 前後空白と複数回のForm描画を文書全体で重複除去する。
#[test]
fn duplicate_page_and_form_lines_are_normalized_once() {
    let parsed = parse_pdf_bytes(
        "duplicate-preview.pdf",
        &pdf(&objects(&shown("  shared  "), &shown("shared"), 2)),
    )
    .unwrap();
    assert_eq!(parsed.text_elements.len(), 3);
    assert_eq!(parsed.text_preview, "shared");
}

/// 異なる局所CMapのcodeを同じUnicodeへ復号した後に重複判定する。
#[test]
fn duplicate_detection_uses_locally_decoded_unicode() {
    let mut objects = objects(
        "BT /F 1 Tf 10 20 Td <4142> Tj ET",
        "BT /F 1 Tf 10 20 Td <4344> Tj ET",
        1,
    );
    objects[9] = "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 12 0 R >>".into();
    objects[10] = "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 13 0 R >>".into();
    objects.push(stream("1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <5171>\n<42> <6709>\nendbfchar"));
    objects.push(stream("1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<43> <5171>\n<44> <6709>\nendbfchar"));
    let parsed = parse_pdf_bytes("decoded-preview.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>(),
        ["共有", "共有"]
    );
    assert_eq!(parsed.text_preview, "共有");
}

/// 復号済みの可視本文は一文字や句読点だけでもpreviewへ保存する。
#[test]
fn decoded_short_and_punctuation_text_is_preserved() {
    for (page, form, expected) in [("A", "文", "A\n文"), ("!", "?", "!\n?")] {
        let parsed = parse_pdf_bytes(
            "short-preview.pdf",
            &pdf(&objects(&shown(page), &shown(form), 1)),
        )
        .unwrap();
        assert_eq!(parsed.text_elements.len(), 2);
        assert_eq!(parsed.text_preview, expected);
    }
}

/// viewport正規化で除かれたページ文字をpreviewへ公開しない。
#[test]
fn preview_uses_visible_page_text() {
    let content = "BT /F 1 Tf 10 20 Td (VISIBLE) Tj 1 0 0 1 20000 20 Tm (HIDDEN) Tj ET";
    let parsed = parse_pdf_bytes("clipped-preview.pdf", &pdf(&objects(content, "", 0))).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "VISIBLE");
    assert_eq!(parsed.text_preview, "VISIBLE");
}

/// ページ本文がすべて不可視でも全体走査のpreviewへfallbackしない。
#[test]
fn fully_clipped_page_has_empty_preview() {
    let content = "BT /F 1 Tf 20000 20 Td (HIDDEN) Tj ET";
    let parsed = parse_pdf_bytes("hidden-preview.pdf", &pdf(&objects(content, "", 0))).unwrap();
    assert!(parsed.text_elements.is_empty());
    assert!(parsed.text_preview.is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 空行・短文・非BMP文字・繰り返しFormの上限を独立した文字列モデルと比較する。
    #[test]
    fn document_preview_matches_unicode_limit_model(left in 0usize..1200, right in 0usize..1200, calls in 1usize..4) {
        let page = "あ".repeat(left);
        let form = "𝄞".repeat(right);
        let expected = [page.as_str(), form.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n").chars().take(1024).collect::<String>();
        let parsed = parse_pdf_bytes("model-preview.pdf", &pdf(&objects(&shown(&page), &shown(&form), calls))).unwrap();
        prop_assert_eq!(&parsed.text_preview, &expected);
        prop_assert!(parsed.text_preview.chars().count() <= 1024);
    }
}
