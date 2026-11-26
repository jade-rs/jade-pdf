//! 完全な原本罫線の列を、セル内の折返しや小さい文字から作った列で置き換えない。
use jade_pdf2docx_docx::infer_ocr_tables_from_pdf_graphics;
use jade_pdf2docx_parser::parse_pdf_bytes;
#[allow(dead_code)]
#[path="../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 右端セルに複数の文字アンカーがある、完全な3列2行の所有表を作る。
fn source()->Vec<u8> {
    let content="1 w 10 100 m 390 100 l S 10 150 m 390 150 l S 10 200 m 390 200 l S 10 100 m 10 200 l S 100 100 m 100 200 l S 300 100 m 300 200 l S 390 100 m 390 200 l S BT /F 10 Tf 20 180 Td (Left) Tj ET BT /F 10 Tf 120 180 Td (Middle) Tj ET BT /F 10 Tf 310 180 Td (90) Tj ET BT /F 10 Tf 350 165 Td (Days) Tj ET BT /F 10 Tf 20 130 Td (Row) Tj ET BT /F 10 Tf 120 130 Td (Value) Tj ET BT /F 10 Tf 310 130 Td (All) Tj ET BT /F 10 Tf 350 115 Td (Text) Tj ET";
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 400 300] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 同じセルの複数行が反復しても、完全に描かれた3列を4列へ変えず、文字の所有を保つ。
#[test]
fn complete_source_columns_own_wrapped_and_offset_text() {
    let parsed=parse_pdf_bytes("owned-authoritative-ruled-columns.pdf",&source()).unwrap();
    let tables=infer_ocr_tables_from_pdf_graphics(&parsed);
    assert_eq!(tables.len(),1);
    let table=&tables[0];
    assert_eq!(table.col_count(),Some(3));
    assert_eq!(table.row_count(),2);
    assert_eq!(table.rows[0][0],"Left");
    assert_eq!(table.rows[0][1],"Middle");
    assert_eq!(table.rows[0][2].split_whitespace().collect::<String>(),"90Days");
    assert_eq!(table.rows[1][0],"Row");
    assert_eq!(table.rows[1][1],"Value");
    assert_eq!(table.rows[1][2].split_whitespace().collect::<String>(),"AllText");
}
