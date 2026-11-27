//! 字形ごとに描いた等幅の原本語を、編集・検索できるWordセルへ戻す。
use jade_pdf2docx_docx::{
    convert_minimum_pdf_bytes_to_docx_bytes, infer_ocr_tables_from_pdf_graphics,
};
use jade_pdf2docx_parser::parse_pdf_bytes;

mod support;
use support::docx_assert::paragraph_texts_in_docx;
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 文字ごとに別の原点を持つCourier表。左上には単語間だけ一文字分の隙間がある。
fn source(multiline: bool) -> Vec<u8> {
    let mut content = String::from(
        "0.5 w 20 160 m 220 160 l S 20 140 m 220 140 l S 20 100 m 220 100 l S \
         20 100 m 20 160 l S 120 100 m 120 160 l S 220 100 m 220 160 l S",
    );
    let mut lines = vec![
        ("Item one", 25, 146),
        ("Qty", 125, 146),
        ("A B", 25, 126),
        ("12", 125, 126),
    ];
    if multiline {
        lines.push(("More text", 25, 110));
    }
    for (text, x, y) in lines {
        for (index, glyph) in text.bytes().enumerate().filter(|(_, ch)| *ch != b' ') {
            let origin = x + 6 * index as i32;
            content.push_str(&format!(
                " BT /F1 10 Tf 1 0 0 1 {origin} {y} Tm ({}) Tj ET",
                char::from(glyph),
            ));
        }
    }
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ])
}

/// 罫線表の公開モデルとDOCXの段落で、字間にない空白を取り除き本物の語間を残す。
#[test]
fn contiguous_courier_glyphs_keep_words_in_editable_cells() {
    let input = source(false);
    let parsed = parse_pdf_bytes("owned-courier-cell.pdf", &input).unwrap();
    let tables = infer_ocr_tables_from_pdf_graphics(&parsed);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].rows, [["Item one", "Qty"], ["A B", "12"]]);

    assert_eq!(
        editable_paragraphs(&input),
        ["Item one", "Qty", "A B", "12"]
    );
}

/// 固定行内にある複数の表示行でも、原本の語間空白を各段落内に残す。
#[test]
fn multiline_courier_cell_keeps_its_word_spaces() {
    assert_eq!(
        editable_paragraphs(&source(true)),
        ["Item one", "Qty", "A B", "More text", "12"]
    );
}

/// 公開のbyte変換からrunをまたぐ段落文字列を取得する。
fn editable_paragraphs(input: &[u8]) -> Vec<String> {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-courier-cell.pdf", input).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("courier-cell.docx");
    std::fs::write(&output, bytes).unwrap();
    paragraph_texts_in_docx(&output)
}
