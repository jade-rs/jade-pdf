//! 原本矩形の背景色を、確認済みの矩形clipと不明な形のclipから区別する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// 外部assetなしの正式PDFで、描画順とclipの範囲を指定する。
fn document(content: &str) -> ParsedDocument {
    let source = fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 200 300] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        fixture::stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ]);
    parse_pdf_bytes("owned-rectangular-background-clip.pdf", &source).unwrap()
}

/// 背景塗りの後の矩形clipが文字全体を含む場合も、確認済みの背景を保持する。
#[test]
fn rectangular_clip_after_fill_keeps_the_verified_background() {
    for rule in ["W", "W*"] {
        let parsed = document(&format!(
            ".851 g 10 100 180 30 re f 0 g q 10 100 180 30 re {rule} n BT /F 12 Tf 20 110 Td (Header) Tj ET Q"
        ));
        assert_eq!(parsed.text_elements.len(), 1);
        let background = parsed.text_elements[0]
            .source_background
            .expect("confirmed rectangle contains the text");
        assert_eq!(background.rgb, [217; 3]);
        assert_eq!(
            [
                background.bounds.x0,
                background.bounds.y0,
                background.bounds.x1,
                background.bounds.y1
            ],
            [10., 100., 190., 130.]
        );
    }
}

/// clipで塗られない範囲を背景に含めず、Q後の別の矩形へclipを漏らさない。
#[test]
fn clipped_fill_uses_visible_intersection_and_q_restores_the_prior_clip() {
    let parsed = document(
        "q 10 100 80 30 re W n .851 g 10 100 180 30 re f 0 g BT /F 12 Tf 20 110 Td (A) Tj ET Q .5 g 10 200 180 30 re f 0 g BT /F 12 Tf 20 210 Td (B) Tj ET",
    );
    assert_eq!(parsed.text_elements.len(), 2);
    for (text, x1, rgb) in [("A", 90., [217; 3]), ("B", 190., [128; 3])] {
        let background = parsed
            .text_elements
            .iter()
            .find(|t| t.text == text)
            .unwrap()
            .source_background
            .expect("verified visible rectangular fill");
        assert_eq!(background.bounds.x0, 10.);
        assert_eq!(background.bounds.x1, x1);
        assert_eq!(background.rgb, rgb);
    }
}

/// 三角形clipを外接矩形へ置き換えて、未描画の領域に背景色を発明しない。
#[test]
fn nonrectangular_clip_remains_unconfirmed() {
    let parsed = document(
        "q 0 0 m 200 0 l 0 300 l h W n .851 g 0 0 200 300 re f 0 g BT /F 12 Tf 20 30 Td (Header) Tj ET Q",
    );
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "Header");
    assert_eq!(parsed.text_elements[0].source_background, None);
}

/// clipのない既存の灰色背景も同じDevice色と原本範囲を保つ。
#[test]
fn unmodified_gray_fill_control_keeps_its_background() {
    let parsed = document(".851 g 10 100 180 30 re f 0 g BT /F 12 Tf 20 110 Td (Header) Tj ET");
    assert_eq!(
        parsed.text_elements[0].source_background.unwrap().rgb,
        [217; 3]
    );
}
