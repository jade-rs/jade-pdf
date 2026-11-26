//! 表示原点・ページ回転を反映した矩形と、公開する基線の座標系を照合する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};

#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// 外部assetを持たない正式PDFで、文字の生の描画座標とページ属性を指定する。
fn document(attributes: &str, content: &str) -> ParsedDocument {
    let source = fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 600 800] {attributes} >>"),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ]);
    parse_pdf_bytes("owned-viewport-baseline.pdf", &source).unwrap()
}

/// 切り抜き原点を差し引く処理は、矩形と確認済みの基線に同じように適用する。
#[test]
fn cropped_baseline_uses_the_same_origin_as_its_bounds() {
    let parsed = document(
        "/CropBox [10 50 410 650]",
        "BT /F 12 Tf 1 0 0 1 30 80 Tm (Visible) Tj ET",
    );
    assert_eq!(parsed.text_elements.len(), 1);
    let text = &parsed.text_elements[0];
    assert_eq!(text.bounds.y0, 30.);
    assert_eq!(text.baseline_y, Some(30.));
}

/// ページ回転で非正立になる文字へ、元の正立基線の確信を残さない。
#[test]
fn rotated_page_does_not_publish_an_unrotated_upright_baseline() {
    for rotation in [90, 180, 270] {
        let parsed = document(
            &format!("/Rotate {rotation}"),
            "BT /F 12 Tf 1 0 0 1 30 80 Tm (Visible) Tj ET",
        );
        assert_eq!(parsed.text_elements.len(), 1);
        assert_eq!(parsed.text_elements[0].text, "Visible");
        assert_eq!(parsed.text_elements[0].baseline_y, None, "rotation {rotation}");
    }
}

/// 原点ゼロで正立したページは、既存の確認済み基線と文字上げを維持する。
#[test]
fn upright_zero_origin_preserves_the_verified_baseline() {
    let parsed = document("", "BT /F 12 Tf 1 0 0 1 30 80 Tm 3 Ts (Visible) Tj ET");
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].baseline_y, Some(83.));
}
