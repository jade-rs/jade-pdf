//! 完全な原本罫線の列を、セル内の折返しや小さい文字から作った列で置き換えない。
use jade_pdf2docx_docx::infer_ocr_tables_from_pdf_graphics;
use jade_pdf2docx_parser::parse_pdf_bytes;
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 右端セルに複数の文字アンカーがある、完全な3列2行の所有表を作る。
fn source() -> Vec<u8> {
    source_with_frame(true, 0, 0, 1)
}

/// 内側罫線のない対照と、同じ罫線・文字を平行移動・拡大した原本を生成する。
fn source_with_frame(internal: bool, dx: u8, dy: u8, scale: u8) -> Vec<u8> {
    let inside = if internal {
        "100 100 m 100 200 l S 300 100 m 300 200 l S"
    } else {
        ""
    };
    let content = format!(
        "q {scale} 0 0 {scale} {dx} {dy} cm 1 w 10 100 m 390 100 l S 10 150 m 390 150 l S 10 200 m 390 200 l S 10 100 m 10 200 l S {inside} 390 100 m 390 200 l S BT /F 10 Tf 20 180 Td (Left) Tj ET BT /F 10 Tf 120 180 Td (Middle) Tj ET BT /F 10 Tf 310 180 Td (90) Tj ET BT /F 10 Tf 350 165 Td (Days) Tj ET BT /F 10 Tf 20 130 Td (Row) Tj ET BT /F 10 Tf 120 130 Td (Value) Tj ET BT /F 10 Tf 310 130 Td (All) Tj ET BT /F 10 Tf 350 115 Td (Text) Tj ET Q"
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 {} {}] >>",
            400 * u16::from(scale) + u16::from(dx),
            300 * u16::from(scale) + u16::from(dy)
        ),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 内側罫線が存在しない外枠では、反復文字列から欠損列を補う既存の経路を維持する。
#[test]
fn outer_frame_without_internal_rules_keeps_the_existing_text_column_route() {
    let parsed = parse_pdf_bytes(
        "owned-missing-internal-rules.pdf",
        &source_with_frame(false, 0, 0, 1),
    )
    .unwrap();
    let tables = infer_ocr_tables_from_pdf_graphics(&parsed);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].col_count(), Some(4));
}

/// native file変換の実際のWordグリッドも、原本の3列と入力byteを維持する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_word_grid_keeps_the_complete_source_columns() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    use std::io::{Cursor, Read};
    let dir = tempfile::tempdir().unwrap();
    let input = source();
    let path = dir.path().join("owned-source.pdf");
    let output = dir.path().join("owned.docx");
    std::fs::write(&path, &input).unwrap();
    convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
        input_pdf_path: path.clone(),
        output_docx_path: output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    })
    .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), input);
    let mut zip = zip::ZipArchive::new(Cursor::new(std::fs::read(output).unwrap())).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(xml.matches("<w:gridCol ").count(), 3);
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 描画全体を移動・拡大しても、実在する3列と各セルの文字所有を維持する。
    #[test]
    fn source_rules_remain_authoritative_under_transforms(dx in 0_u8..=40,dy in 0_u8..=40,scale in 1_u8..=3) {
        let parsed=parse_pdf_bytes("owned-transformed-ruled-columns.pdf",&source_with_frame(true,dx,dy,scale)).unwrap();
        let tables=infer_ocr_tables_from_pdf_graphics(&parsed);
        prop_assert_eq!(tables.len(),1);
        prop_assert_eq!(tables[0].col_count(),Some(3));
        prop_assert_eq!(&tables[0].rows[0][0],"Left");
        prop_assert_eq!(&tables[0].rows[0][1],"Middle");
        prop_assert_eq!(tables[0].rows[0][2].split_whitespace().collect::<String>(),"90Days");
    }
}

/// 同じセルの複数行が反復しても、完全に描かれた3列を4列へ変えず、文字の所有を保つ。
#[test]
fn complete_source_columns_own_wrapped_and_offset_text() {
    let parsed = parse_pdf_bytes("owned-authoritative-ruled-columns.pdf", &source()).unwrap();
    let tables = infer_ocr_tables_from_pdf_graphics(&parsed);
    assert_eq!(tables.len(), 1);
    let table = &tables[0];
    assert_eq!(table.col_count(), Some(3));
    assert_eq!(table.row_count(), 2);
    assert_eq!(table.rows[0][0], "Left");
    assert_eq!(table.rows[0][1], "Middle");
    assert_eq!(
        table.rows[0][2].split_whitespace().collect::<String>(),
        "90Days"
    );
    assert_eq!(table.rows[1][0], "Row");
    assert_eq!(table.rows[1][1], "Value");
    assert_eq!(
        table.rows[1][2].split_whitespace().collect::<String>(),
        "AllText"
    );
}
