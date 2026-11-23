//! 編集可能な本文が残っても、再現しない曲線描画の欠落を無警告で成功にしない。
use jade_pdf2docx_parser::{ParseOptions, parse_pdf_bytes, parse_pdf_bytes_with_options};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 読める本文と、文字とは独立した任意のpathを持つ正式な一ページPDFを作る。
fn source(path: &str) -> Vec<u8> {
    let content = format!("BT /F1 10 Tf 20 80 Td (VISIBLE TEXT) Tj ET {path}");
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        page_attribute_pdf::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 一つのページに記録された、曲線の未再現診断の数を公開表示から取得する。
fn losses(parsed: &jade_pdf2docx_parser::ParsedDocument) -> usize {
    parsed
        .quality
        .issues
        .iter()
        .filter(|issue| issue.to_string().contains("unsupported_curved_path"))
        .count()
}

/// 塗り・線・両方の曲線描画は、既存文字の存在にかかわらず未再現を報告する。
#[test]
fn painted_curve_loss_is_reported_without_losing_existing_text() {
    for curve in ["20 40 40 40 60 10 c", "40 40 60 10 v", "40 40 60 10 y"] {
        for paint in ["S", "s", "f", "F", "f*", "B", "B*", "b", "b*"] {
            let bytes = source(&format!("10 10 m {curve} {paint}"));
            let parsed = parse_pdf_bytes("owned-curve.pdf", &bytes).unwrap();
            assert!(parsed.text_preview.contains("VISIBLE TEXT"));
            assert_eq!(losses(&parsed), 1, "{curve} {paint}: {:?}", parsed.quality);
        }
    }
}

/// 描画を取り消したpath、文字列・配列内の偽命令、通常罫線を曲線の欠落にしない。
#[test]
fn unpainted_paths_and_literal_operators_keep_the_control() {
    for path in [
        "10 10 m 20 40 40 40 60 10 c n",
        "10 10 m 20 40 40 40 60 10 c n 10 10 m 60 10 l S",
        "BT /F1 10 Tf 20 60 Td (c v y f S) Tj ET",
        "[(c) (v) (y) (f)] TJ /c /f /S",
        "% 10 10 m 20 40 40 40 60 10 c f\n10 10 m 60 10 l S",
    ] {
        let parsed = parse_pdf_bytes("owned-curve-control.pdf", &source(path)).unwrap();
        assert_eq!(losses(&parsed), 0, "{path}: {:?}", parsed.quality);
    }
}

/// strictは本文のあるページでも、未再現の曲線を無視した成功を返さない。
#[test]
fn strict_rejects_unrepresented_curves_before_publication() {
    let result = parse_pdf_bytes_with_options(
        "owned-strict-curve.pdf",
        &source("10 10 m 20 40 40 40 60 10 c f"),
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    );
    let error = result.expect_err("an unrepresented painted curve must reject strict conversion");
    assert!(
        error.to_string().contains("unsupported_curved_path"),
        "{error}"
    );
}

/// 曲線の終点を無視して、その前のmoveから存在しない水平罫線を生成しない。
#[test]
fn curve_does_not_invent_a_straight_table_rule() {
    let parsed = parse_pdf_bytes(
        "owned-curve-rule.pdf",
        &source("10 10 m 20 40 40 40 60 40 c 60 10 l S"),
    )
    .unwrap();
    assert!(
        !parsed.graphics.iter().any(|line| {
            line.bounds.x0 == 10.
                && line.bounds.x1 == 60.
                && line.bounds.y0 == 10.
                && line.bounds.y1 == 10.
        }),
        "a curved source path became a fictitious straight rule: {:?}",
        parsed.graphics
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 同一ページで曲線を繰り返しても、診断の欠落・重複や既存本文の欠落がない。
    #[test]
    fn repeated_painted_curves_have_one_page_diagnostic(repeats in 1usize..32) {
        let bytes=source(&"10 10 m 20 40 40 40 60 10 c f ".repeat(repeats));
        let parsed=parse_pdf_bytes("owned-repeated-curves.pdf",&bytes).unwrap();
        prop_assert_eq!(losses(&parsed),1);
        prop_assert!(parsed.text_preview.contains("VISIBLE TEXT"));
    }
}
