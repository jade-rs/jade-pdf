//! 非派生fontでも確認済みの正立描画行列から基線を保存する公開API回帰。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// font経路とForm変換を選び、外部fontを保存せず正式な所有PDFを作る。
fn document(kind: u8, content: &str, in_form: bool) -> ParsedDocument {
    let font = match kind {
        0 => "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
        1 => format!("<< /Type /Font /Subtype /TrueType /BaseFont /OwnedSans /Encoding /WinAnsiEncoding /FirstChar 0 /LastChar 255 /Widths [{}] >>", vec!["600";256].join(" ")),
        2 => "<< /Type /Font /Subtype /Type0 /BaseFont /OwnedCID /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".into(),
        _ => unreachable!(),
    };
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 600 800] /Resources << /Font << /F 5 0 R >> /XObject << /Fm 8 0 R >> >> >>".into(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into(),
        fixture::stream(if in_form { "/Fm Do" } else { content }),
        font,
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /OwnedCID /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 600 >>".into(),
        fixture::stream("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n4 beginbfchar\n<0041> <0041>\n<0032> <0032>\n<0042> <0042>\n<0043> <0043>\nendbfchar"),
        format!("<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] /Matrix [2 0 0 3 7 11] /Resources << /Font << /F 5 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",content.len()),
    ];
    parse_pdf_bytes("owned-upright-baseline.pdf", &fixture::pdf(&objects)).unwrap()
}

/// 各font経路で通常文字と上付き文字の独立した基線・文字サイズを照合する。
fn assert_baselines(kind: u8) {
    let a = if kind == 2 { "<0041>" } else { "(A)" };
    let two = if kind == 2 { "<0032>" } else { "(2)" };
    let content =
        format!("BT /F 18 Tf 1 0 0 1 30 80 Tm {a} Tj /F 10 Tf 1 0 0 1 41 86 Tm {two} Tj ET");
    for in_form in [false, true] {
        let parsed = document(kind, &content, in_form);
        assert_eq!(parsed.text_elements.len(), 2);
        assert_eq!(parsed.text_elements[0].text, "A");
        assert_eq!(parsed.text_elements[1].text, "2");
        let (scale, origin) = if in_form { (3., 11.) } else { (1., 0.) };
        for (element, baseline, size) in [
            (&parsed.text_elements[0], 80., 18.),
            (&parsed.text_elements[1], 86., 10.),
        ] {
            assert_eq!(element.font_size, size * scale);
            assert_eq!(
                element.baseline_y,
                Some(baseline * scale + origin),
                "upright source baseline must survive independently of font derivation"
            );
        }
    }
}

/// Standard14の描画原点は派生Type1プログラムがなくても既知である。
#[test]
fn standard14_keeps_verified_source_baseline() {
    assert_baselines(0);
}

/// TrueTypeの描画原点をfontファイルの許諾や字形の推測と切り離して保存する。
#[test]
fn truetype_keeps_verified_source_baseline() {
    assert_baselines(1);
}

/// Type0/CID経路も同じ正立行列とForm変換を使う。
#[test]
fn cid_keeps_verified_source_baseline() {
    assert_baselines(2);
}
