//! 表の外枠を直前の本文見出しの下線にせず、独立した下線を区別する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 見出しの4pt下にある正式な二列の表と、任意の独立下線を作る。
fn source(independent: bool) -> Vec<u8> {
    transformed_source(independent, true, 0., 0., 1.)
}

/// 完全でない外枠と原本の一様変換も、同じ正式なPDF構造で作る。
fn transformed_source(
    independent: bool,
    right_edge: bool,
    dx: f32,
    dy: f32,
    scale: f32,
) -> Vec<u8> {
    let underline = if independent {
        "82 652 m 124 652 l S"
    } else {
        ""
    };
    let right = if right_edge {
        "272 600 m 272 650 l S"
    } else {
        ""
    };
    let content = format!(
        "q {scale} 0 0 {scale} {dx} {dy} cm 1 w 72 600 m 272 600 l S 72 650 m 272 650 l S 72 600 m 72 650 l S 172 600 m 172 650 l S {right} {underline} BT /F 10 Tf 82 654 Td (Heading) Tj ET BT /F 10 Tf 82 625 Td (Alpha) Tj ET BT /F 10 Tf 182 625 Td (Control) Tj ET Q"
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 表の上辺に近くても、別に描かれた見出しの実下線は保持する。
#[test]
fn independent_body_underline_survives_next_to_the_table_frame() {
    let output = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-independent-heading-underline.pdf",
        &source(true),
    )
    .unwrap());
    assert!(run(&output, "Heading").contains("<w:u "));
}

/// 四辺の根拠が揃わない枠には、新しい除去を適用しない。
#[test]
fn an_unconfirmed_open_frame_keeps_the_previous_underline_route() {
    let output = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-incomplete-frame.pdf",
        &transformed_source(false, false, 0., 0., 1.),
    )
    .unwrap());
    assert!(run(&output, "Heading").contains("<w:u "));
}

/// native file経路でも偽の下線を除き、実線の対照と元の入力を保持する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_heading_uses_the_verified_frame_without_mutating_the_input() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    for independent in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let bytes = source(independent);
        let input = dir.path().join("owned.pdf");
        let dest = dir.path().join("owned.docx");
        std::fs::write(&input, &bytes).unwrap();
        convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
            input_pdf_path: input.clone(),
            output_docx_path: dest.clone(),
            ocr_text_lines: None,
            ocr_table: None,
            clean_output: true,
        })
        .unwrap();
        assert_eq!(std::fs::read(input).unwrap(), bytes);
        let output = xml(std::fs::read(dest).unwrap());
        assert_eq!(run(&output, "Heading").contains("<w:u "), independent);
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 原本の平行移動と一様拡縮でも、閉じた表と独立下線を取り違えない。
    #[test]
    fn body_underline_roles_follow_source_frame_transforms(independent in any::<bool>(),dx in 0_f32..20.,dy in 0_f32..15.,scale in 0.8_f32..1.05) {
        let output=xml(convert_minimum_pdf_bytes_to_docx_bytes("owned-heading-frame-transform.pdf",&transformed_source(independent,true,dx,dy,scale)).unwrap());
        prop_assert_eq!(run(&output,"Heading").contains("<w:u "),independent);
    }
}

/// 製品の本文XMLだけを返す。
fn xml(output: Vec<u8>) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut s = String::new();
    z.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

/// 対象ラベルのrunだけを検査し、table borderと下線属性を混同しない。
fn run<'a>(xml: &'a str, label: &str) -> &'a str {
    let p = xml.find(&format!("{label}</w:t>")).unwrap();
    let start = xml[..p].rfind("<w:r>").unwrap();
    let end = p + xml[p..].find("</w:r>").unwrap() + "</w:r>".len();
    &xml[start..end]
}

/// 表の完全な上枠は、表外の見出しへ下線を新設する根拠にしない。
#[test]
fn a_complete_source_table_top_rule_does_not_underline_the_body_heading() {
    let output = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-body-heading-table-rule.pdf",
        &source(false),
    )
    .unwrap());
    assert_eq!(output.matches("<w:tbl>").count(), 1);
    assert!(
        !run(&output, "Heading").contains("<w:u "),
        "table frame is not a body underline: {}",
        run(&output, "Heading")
    );
}
