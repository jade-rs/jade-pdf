//! 標準太字faceの幅を通常体・Unicode概算で代用しない回帰。
use super::*;
use crate::decode::DecodeBudget;

/// 標準Type1の直接辞書から、12ptのencoded glyph advanceを読む。
fn advance(name: &str, encoding: &str, extra: &str, operand: &[u8]) -> Option<f32> {
    let dict = format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name} {encoding} {extra} >>");
    let budget = DecodeBudget::default();
    Metrics::read(&Objects::new(), dict.as_bytes(), &budget, &[])
        .layout(&budget, operand, 12., 0., 0.)
        .map(|layout| layout.advance)
}

/// Adobe AFMのR=722・digit=556・C=722を使い、混在セルの太字末尾を短く見積もらない。
#[test]
fn standard_bold_uses_its_own_glyph_advances() {
    for font in ["Helvetica-Bold", "Helvetica-BoldOblique"] {
        let actual = advance(font, "/Encoding /WinAnsiEncoding", "", b"(R0C1)")
            .expect("standard bold metrics");
        assert!((actual - 30.672).abs() < 0.0001, "{font}: {actual}");
    }
}

/// StandardEncodingの左右引用符とASCII apostrophe/graveを区別する。
#[test]
fn standard_bold_quotes_follow_the_source_encoding() {
    for encoding in ["", "/Encoding /StandardEncoding"] {
        assert!((advance("Helvetica-Bold", encoding, "", b"('`)").unwrap() - 6.672).abs() < 0.0001);
    }
    for encoding in ["/Encoding /WinAnsiEncoding", "/Encoding /MacRomanEncoding"] {
        assert!((advance("Helvetica-Bold", encoding, "", b"('`)").unwrap() - 6.852).abs() < 0.0001);
    }
}

/// 明示Widthsを優先し、再符号化されたfont・別名・未知glyphへ標準値を流用しない。
#[test]
fn bold_standard_metrics_do_not_override_source_widths_or_differences() {
    assert_eq!(
        advance("Helvetica-Bold", "", "/FirstChar 82 /Widths [1000]", b"(R)"),
        Some(12.)
    );
    assert!(
        advance(
            "Helvetica-Bold",
            "/Encoding << /Differences [82 /A] >>",
            "",
            b"(R)"
        )
        .is_none()
    );
    assert!(advance("ABCDEF+Helvetica-Bold", "", "", b"(R)").is_none());
    assert!(advance("Helvetica-Bold", "", "", b"<ff>").is_none());
}
