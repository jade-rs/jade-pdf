//! 確認済みの矩形clipの下で、原本全体を覆うセル背景だけをWordへ保存する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 同じ正式な2列2行の表で、背景塗り自体が部分clipされる対照も作る。
fn source(partial_fill: bool) -> Vec<u8> {
    let background = if partial_fill {
        "q 10 120 80 20 re W n .851 g 10 120 180 20 re f Q 0 g"
    } else {
        ".851 g 10 120 180 20 re f 0 g"
    };
    let content = format!(
        "{background} 1 w 10 100 m 190 100 l S 10 120 m 190 120 l S 10 140 m 190 140 l S 10 100 m 10 140 l S 100 100 m 100 140 l S 190 100 m 190 140 l S q 10 120 90 20 re W n BT /F 10 Tf 20 125 Td (H1) Tj ET Q q 100 120 90 20 re W* n BT /F 10 Tf 110 125 Td (H2) Tj ET Q BT /F 10 Tf 20 105 Td (A) Tj ET BT /F 10 Tf 110 105 Td (B) Tj ET"
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 200 300] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 公開変換が作った本文XMLを、file/bytes両経路で同じように検査する。
fn xml(output: Vec<u8>) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut text = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// clipのある文字でも、原本全体が灰色の見出し2セルだけを塗る。
#[test]
fn header_cells_keep_gray_under_verified_text_clips() {
    let output =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-clipped-header.pdf", &source(false))
            .unwrap();
    let text = xml(output);
    assert_eq!(text.matches("w:fill=\"D9D9D9\"").count(), 2);
    for value in ["H1", "H2", "A", "B"] {
        assert!(text.contains(&format!(">{value}</w:t>")));
    }
}

/// 部分的にしか塗られていないセルを、推測で全体の灰色へ変えない。
#[test]
fn partially_clipped_fill_does_not_become_a_whole_cell_background() {
    let output =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-partial-header-fill.pdf", &source(true))
            .unwrap();
    assert_eq!(xml(output).matches("w:fill=\"D9D9D9\"").count(), 0);
}

/// native file経路でも同じ2セルの原本色を保ち、入力を変更しない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_header_cells_keep_the_verified_clipped_background() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    let dir = tempfile::tempdir().unwrap();
    let input = source(false);
    let path = dir.path().join("owned-source.pdf");
    let dest = dir.path().join("owned.docx");
    std::fs::write(&path, &input).unwrap();
    convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
        input_pdf_path: path.clone(),
        output_docx_path: dest.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    })
    .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), input);
    assert_eq!(
        xml(std::fs::read(dest).unwrap())
            .matches("w:fill=\"D9D9D9\"")
            .count(),
        2
    );
}
