//! `/Rotate` 付きページで native(Pdfium) 抽出のテキストが罫線と同じ表示座標になることを確認する。
#![cfg(not(target_arch = "wasm32"))]

use jade_pdf2docx_parser::{ParsedTextElement, parse_pdf, parse_pdf_bytes};

mod support;
use support::paths::temp_file_path;

/// 正確な xref を持つ、回転ページ 1 枚の PDF を作る。
fn rotated_page_pdf(rotation: i32, content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Rotate {rotation} /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut source = "%PDF-1.4\n".to_owned();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = source.len();
    source.push_str("xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    source.into_bytes()
}

/// 表示上で左から右へ読む横書きを、`/Rotate 270` の用紙へ 90 度回して描いた本文と罫線。
///
/// NUL 文字は pure Rust 抽出と Pdfium で文字数が合わず、Pdfium の幾何をそのまま使う経路になる。
const ROTATED_CONTENT: &str =
    "BT /F1 12 Tf 0 -1 1 0 50 250 Tm (HELLO\\000WORLD) Tj ET 0.5 w 40 260 m 40 100 l S";

/// file入力（Pdfium文字を使う経路）で解析する。
fn parse_native(name: &str, bytes: &[u8]) -> Vec<ParsedTextElement> {
    let path = temp_file_path(name);
    std::fs::write(&path, bytes).unwrap();
    let parsed = parse_pdf(&path).expect("rotated page must parse");
    let _ = std::fs::remove_file(path);
    parsed.text_elements
}

/// 表示座標で横長の行として、pure Rust 抽出と同じ位置に置かれる。
#[test]
fn native_text_on_rotated_page_uses_display_coordinates() {
    let bytes = rotated_page_pdf(270, ROTATED_CONTENT);
    let pure = parse_pdf_bytes("rotated-pure.pdf", &bytes).expect("pure parse");
    let native = parse_native("rotated_native_text", &bytes);

    let pure_text = pure
        .text_elements
        .iter()
        .find(|text| text.text.contains("HELLO"))
        .expect("pure text");
    let native_text = native
        .iter()
        .find(|text| text.text.contains("HELLO"))
        .expect("native text");
    let width = native_text.bounds.x1 - native_text.bounds.x0;
    let height = native_text.bounds.y1 - native_text.bounds.y0;
    assert!(
        width > height * 3.0,
        "display-horizontal line must be wide, got {:?}",
        native_text.bounds
    );
    let pure_center_y = (pure_text.bounds.y0 + pure_text.bounds.y1) / 2.0;
    let native_center_y = (native_text.bounds.y0 + native_text.bounds.y1) / 2.0;
    assert!(
        (pure_center_y - native_center_y).abs() < 6.0,
        "native {:?} must share the display baseline with pure {:?}",
        native_text.bounds,
        pure_text.bounds
    );
    assert!(
        (pure_text.bounds.x0 - native_text.bounds.x0).abs() < 6.0,
        "native {:?} must start where pure {:?} starts",
        native_text.bounds,
        pure_text.bounds
    );
}

/// 回転した文字行列でも font size を 0 にしない。
#[test]
fn native_text_on_rotated_page_keeps_font_size() {
    let bytes = rotated_page_pdf(270, ROTATED_CONTENT);
    let native = parse_native("rotated_native_font_size", &bytes);
    let text = native
        .iter()
        .find(|text| text.text.contains("HELLO"))
        .expect("native text");
    assert!(
        (text.font_size - 12.0).abs() < 1.0,
        "font size must stay 12pt, got {}",
        text.font_size
    );
}
