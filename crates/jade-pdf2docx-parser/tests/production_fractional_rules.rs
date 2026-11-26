//! 原本罫線の小数座標を、表推定用の整数キーへ丸めて失わない。
use jade_pdf2docx_parser::parse_pdf_bytes;

#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 小数座標の線と四辺を持つ正式な一ページPDFを生成する。
fn source(content: &str) -> Vec<u8> {
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 500 500] /Contents 4 0 R /Resources << >> >>".into(),
        page_attribute_pdf::stream(content),
    ])
}

/// 四分の一点の原本矩形は、解析公開値でも四辺すべて同じ小数座標を保持する。
#[test]
fn source_rectangle_keeps_fractional_edges_before_table_inference() {
    let parsed = parse_pdf_bytes("owned-fractional-rules.pdf", &source(".5 w 20.25 30.75 40.5 10.5 re S")).unwrap();
    let actual: Vec<_> = parsed.graphics.iter().map(|g| [g.bounds.x0,g.bounds.y0,g.bounds.x1,g.bounds.y1]).collect();
    assert_eq!(actual, vec![[20.25,30.75,20.25,41.25],[20.25,30.75,60.75,30.75],[20.25,41.25,60.75,41.25],[60.75,30.75,60.75,41.25]], "the parser must not replace source coordinates with integer grid keys");
}
