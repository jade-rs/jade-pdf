//! 数値列の大きいTJ間隔を、一つの横長本文矩形へ潰さず原本位置を保つ。
use jade_pdf2docx_parser::parse_pdf_bytes;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 所有するCourier原本に、明示的な30pointの列間隔を持つ二つの値を置く。
fn source() -> Vec<u8> {
    let mut objects = pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> >>");
    objects[4] = stream("BT /F 10 Tf 20 100 Td [(376) -3000 (-2.4)] TJ ET");
    objects[5] = stream("");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    pdf(&objects)
}

/// 表セルへの割当前に、別々の原本数値と各原点・幅を公開byte APIで確認する。
#[test]
fn wide_numeric_tj_gap_preserves_independent_source_positions() {
    let parsed = parse_pdf_bytes("owned-numeric-tj.pdf", &source()).unwrap();
    let elements = parsed.text_elements.iter().filter(|t| !t.text.trim().is_empty()).collect::<Vec<_>>();
    assert_eq!(elements.len(), 2, "two numeric columns must retain independent source frames");
    assert_eq!(elements[0].text, "376");
    assert_eq!(elements[1].text, "-2.4");
    assert_eq!((elements[0].bounds.x0, elements[0].bounds.x1), (20., 38.));
    assert_eq!((elements[1].bounds.x0, elements[1].bounds.x1), (68., 92.));
    assert_eq!(elements[0].baseline_y, elements[1].baseline_y);
}
