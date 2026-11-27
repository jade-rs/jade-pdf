//! 表示原点・ページ回転を反映した矩形と、公開する基線の座標系を照合する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};

#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// 外部assetを持たない正式PDFで、文字の生の描画座標とページ属性を指定する。
fn document(attributes: &str, content: &str) -> ParsedDocument {
    document_in_frame("[0 0 600 800]", attributes, content)
}

/// MediaBoxの原点も任意に指定して、継承された有効表示原点を確認する。
fn document_in_frame(media: &str, attributes: &str, content: &str) -> ParsedDocument {
    let source = fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox {media} {attributes} >>"),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
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

/// CropBoxがなくても、正負のMediaBox原点を矩形と基線へ一度だけ反映する。
#[test]
fn media_box_origin_is_applied_without_a_crop_box() {
    for (media, expected) in [("[10 50 610 850]", 30.), ("[-10 -50 590 750]", 130.)] {
        let parsed = document_in_frame(media, "", "BT /F 12 Tf 1 0 0 1 30 80 Tm (Visible) Tj ET");
        assert_eq!(parsed.text_elements.len(), 1);
        assert_eq!(parsed.text_elements[0].baseline_y, Some(expected));
        assert_eq!(parsed.text_elements[0].bounds.y0, expected);
    }
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
        assert_eq!(
            parsed.text_elements[0].baseline_y, None,
            "rotation {rotation}"
        );
    }
}

/// 原点ゼロで正立したページは、既存の確認済み基線と文字上げを維持する。
#[test]
fn upright_zero_origin_preserves_the_verified_baseline() {
    let parsed = document("", "BT /F 12 Tf 1 0 0 1 30 80 Tm 3 Ts (Visible) Tj ET");
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].baseline_y, Some(83.));
}

/// 一部表示された字面の基線が表示枠の外にあっても、枠端へ丸めて偽の位置を作らない。
#[test]
fn partially_clipped_text_keeps_its_unclamped_relative_baseline() {
    let parsed = document(
        "/CropBox [10 50 410 650]",
        "BT /F 12 Tf 1 0 0 1 30 45 Tm (Visible) Tj ET",
    );
    assert_eq!(parsed.text_elements.len(), 1);
    let text = &parsed.text_elements[0];
    assert_eq!(text.bounds.y0, 0.);
    assert_eq!(text.bounds.y1, 7.);
    assert_eq!(text.baseline_y, Some(-5.));
}

/// 先に合成したForm行列と文字上げへ、最後に表示原点の平行移動を適用する。
#[test]
fn form_and_rise_baselines_receive_the_page_origin_once() {
    let content = "BT /F 12 Tf 1 0 0 1 30 80 Tm 3 Ts (Visible) Tj ET";
    let source = fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 600 800] /CropBox [10 50 410 650] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /XObject << /Fm 6 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream("/Fm Do"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
        format!("<< /Type /XObject /Subtype /Form /BBox [0 0 300 300] /Matrix [2 0 0 3 7 11] /Resources << /Font << /F 5 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",content.len()),
    ]);
    let parsed = parse_pdf_bytes("owned-form-viewport-baseline.pdf", &source).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].baseline_y, Some(210.));
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// ページの切り抜きと文字を一緒に平行移動しても、表示内の全文字情報は変わらない。
    #[test]
    fn crop_and_text_translation_preserve_the_same_display_element(
        x in 0_u8..=100, y in 0_u8..=100, size in 4_u8..=20, rise in -4_i8..=4,
        turn in 0_u8..=3,
    ) {
        let rotation=u16::from(turn)*90;
        let original=document(
            &format!("/CropBox [0 0 400 600] /Rotate {rotation}"),
            &format!("BT /F {size} Tf 1 0 0 1 30 80 Tm {rise} Ts (Visible) Tj ET"),
        );
        let moved=document(
            &format!("/CropBox [{x} {y} {} {}] /Rotate {rotation}",u16::from(x)+400,u16::from(y)+600),
            &format!("BT /F {size} Tf 1 0 0 1 {} {} Tm {rise} Ts (Visible) Tj ET",u16::from(x)+30,u16::from(y)+80),
        );
        prop_assert_eq!(moved.text_elements.len(),1);
        prop_assert_eq!(original.text_elements.len(),1);
        let mut moved_text=moved.text_elements[0].clone();
        let original_text=&original.text_elements[0];
        for (a,b) in [
            (moved_text.bounds.x0,original_text.bounds.x0),
            (moved_text.bounds.x1,original_text.bounds.x1),
            (moved_text.bounds.y0,original_text.bounds.y0),
            (moved_text.bounds.y1,original_text.bounds.y1),
        ] {
            // 平行移動のf32加減算だけに生じる丸めを許し、基線と全styleは厳密に照合する。
            prop_assert!((a-b).abs()<=0.0001,"bounds differ: {} vs {}",a,b);
        }
        moved_text.bounds=original_text.bounds;
        prop_assert_eq!(&moved_text,original_text);
        prop_assert_eq!(moved.text_elements[0].baseline_y,if turn==0 {Some(80.+f32::from(rise))} else {None});
    }
}
