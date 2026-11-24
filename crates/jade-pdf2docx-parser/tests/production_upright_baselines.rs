//! 非派生fontでも確認済みの正立描画行列から基線を保存する公開API回帰。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// font経路とForm変換を選び、外部fontを保存せず正式な所有PDFを作る。
fn document(kind: u8, content: &str, in_form: bool) -> ParsedDocument {
    let font = match kind {
        0 => "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
        1 => format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /OwnedSans /Encoding /WinAnsiEncoding /FirstChar 0 /LastChar 255 /Widths [{}] >>",
            vec!["600"; 256].join(" ")
        ),
        2..=6 => {
            let encoding = match kind {
                2 => "/Identity-H",
                3 => "/Identity-V",
                4 | 6 => "9 0 R",
                _ => "/Unknown-CMap",
            };
            format!(
                "<< /Type /Font /Subtype /Type0 /BaseFont /OwnedCID /Encoding {encoding} /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>"
            )
        }
        _ => unreachable!(),
    };
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 600 800] /Resources << /Font << /F 5 0 R /G 10 0 R >> /XObject << /Fm 8 0 R >> >> >>".into(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into(),
        fixture::stream(if in_form { "/Fm Do" } else { content }),
        font,
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /OwnedCID /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 600 >>".into(),
        fixture::stream("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n4 beginbfchar\n<0041> <0041>\n<0032> <0032>\n<0042> <0042>\n<0043> <0043>\nendbfchar"),
        format!("<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] /Matrix [2 0 0 3 7 11] /Resources << /Font << /F 5 0 R /G 10 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",content.len()),
        fixture::stream(&format!("/WMode {} def 1 begincodespacerange <0000> <FFFF> endcodespacerange 4 begincidchar <0041> 65 <0032> 50 <0042> 66 <0043> 67 endcidchar", if kind==6 {0} else {1})),
        "<< /Type /Font /Subtype /Type0 /BaseFont /OwnedCID /Encoding /Identity-V /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".into(),
    ];
    parse_pdf_bytes("owned-upright-baseline.pdf", &fixture::pdf(&objects)).unwrap()
}

/// 各font経路で通常文字と上付き文字の独立した基線・文字サイズを照合する。
fn assert_baselines(kind: u8) {
    let a = if kind >= 2 { "<0041>" } else { "(A)" };
    let two = if kind >= 2 { "<0032>" } else { "(2)" };
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

/// 同一基線のTJ断片と隣接描画は、従来のspan結合と本文を保つ。
#[test]
fn coherent_text_spans_keep_their_source_baseline_when_merged() {
    for kind in 0..=2 {
        let (a, b, c) = if kind >= 2 {
            ("<0041>", "<0042>", "<0043>")
        } else {
            ("(A)", "(B)", "(C)")
        };
        let parsed = document(
            kind,
            &format!("BT /F 18 Tf 1 0 0 1 30 80 Tm [{a} 0 {b}] TJ {c} Tj ET"),
            false,
        );
        assert_eq!(parsed.text_elements.len(), 1);
        assert_eq!(parsed.text_elements[0].text, "ABC");
        assert_eq!(parsed.text_elements[0].baseline_y, Some(80.));
    }
}

/// 文字上げを変えた同じfontの描画を別spanに保ち、各描画原点を区別する。
#[test]
fn text_rise_preserves_distinct_baselines() {
    let parsed = document(
        0,
        "BT /F 18 Tf 1 0 0 1 30 80 Tm (A) Tj 6 Ts (B) Tj 0 Ts (C) Tj ET",
        false,
    );
    assert_eq!(parsed.text_elements.len(), 3);
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| t.baseline_y)
            .collect::<Vec<_>>(),
        [Some(80.), Some(86.), Some(80.)]
    );
}

/// 回転・反転・斜交と負の文字サイズを、正立文字の基線として公開しない。
#[test]
fn unverified_axes_and_nonpositive_sizes_do_not_gain_upright_baselines() {
    for matrix in ["0 1 -1 0", "-1 0 0 1", "1 0 0 -1", "1 0.1 0 1", "1 0 0.1 1"] {
        for in_form in [false, true] {
            let parsed = document(
                0,
                &format!("BT /F 18 Tf {matrix} 80 80 Tm (A) Tj ET"),
                in_form,
            );
            assert_eq!(parsed.text_elements.len(), 1);
            assert!(parsed.text_elements[0].baseline_y.is_none());
        }
    }
    let parsed = document(0, "BT /F -18 Tf 1 0 0 1 80 80 Tm (A) Tj ET", false);
    assert_eq!(parsed.text_elements.len(), 1);
    assert!(parsed.text_elements[0].baseline_y.is_none());
}

/// 局所行列が反転していても、CTM合成後に正立した文字の基線は既知である。
#[test]
fn two_reflections_are_verified_in_page_space() {
    let parsed = document(
        0,
        "q -1 0 0 -1 200 200 cm BT /F 18 Tf -1 0 0 -1 30 80 Tm (A) Tj ET Q",
        false,
    );
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].baseline_y, Some(120.));
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    /// Font経路・平行移動・縦横倍率・文字上げ・Formを合成しても、基線は同じページ座標になる。
    #[test]
    fn source_matrix_baseline_is_font_independent(
        kind_index in 0_u8..=3, in_form in any::<bool>(),
        x in 30_u8..=60, y in 50_u8..=90,
        sx in 1_u8..=2, sy in 1_u8..=2, size in 4_u8..=20, rise in -5_i8..=5,
    ) {
        let kind = if kind_index == 3 {6} else {kind_index};
        let a = if kind >= 2 { "<0041>" } else { "(A)" };
        let content = format!("BT /F {size} Tf {sx} 0 0 {sy} {x} {y} Tm {rise} Ts {a} Tj ET");
        let parsed = document(kind,&content,in_form);
        prop_assert_eq!(parsed.text_elements.len(),1);
        let (scale,origin) = if in_form { (3.,11.) } else { (1.,0.) };
        let expected = (f32::from(y)+f32::from(sy)*f32::from(rise))*scale+origin;
        prop_assert_eq!(parsed.text_elements[0].baseline_y,Some(expected));
        prop_assert_eq!(parsed.text_elements[0].font_size,f32::from(size)*f32::from(sy)*scale);
        prop_assert_eq!(parsed.text_elements[0].text.as_str(),"A");
    }
}

/// 正立した文字行列でも、縦書き・未確認Encodingを水平基線と取り違えない。
#[test]
fn vertical_and_unverified_cmaps_do_not_gain_horizontal_baselines() {
    for kind in 3..=5 {
        for in_form in [false, true] {
            let parsed = document(kind, "BT /F 18 Tf 1 0 0 1 30 80 Tm <0041> Tj ET", in_form);
            assert_eq!(parsed.text_elements.len(), 1);
            assert_eq!(parsed.text_elements[0].text, "A");
            assert!(
                parsed.text_elements[0].baseline_y.is_none(),
                "vertical or unverified writing mode must not gain a horizontal baseline"
            );
        }
    }
}

/// 同名の水平・垂直font aliasを切り替えた断片へ、隣の確認済み基線を伝播しない。
#[test]
fn merged_aliases_do_not_transfer_baseline_confidence() {
    for reverse in [false, true] {
        let (first, second) = if reverse { ("G", "F") } else { ("F", "G") };
        let parsed = document(
            2,
            &format!("BT /{first} 18 Tf 1 0 0 1 30 80 Tm <0041> Tj /{second} 18 Tf <0042> Tj ET"),
            false,
        );
        assert_eq!(
            parsed.text_elements.len(),
            2,
            "horizontal and vertical aliases must not merge baseline confidence"
        );
        assert_eq!(parsed.text_elements[0].text, "A");
        assert_eq!(parsed.text_elements[1].text, "B");
        let actual = parsed
            .text_elements
            .iter()
            .map(|t| t.baseline_y)
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            if reverse {
                vec![None, Some(80.)]
            } else {
                vec![Some(80.), None]
            }
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 同名aliasを入れ替え、移動・文字上げ・Formを変えても、水平側だけが既知の基線を持つ。
    #[test]
    fn mixed_writing_aliases_preserve_confidence_under_transforms(
        reverse in any::<bool>(), in_form in any::<bool>(),
        x in 30_u8..=60, y in 50_u8..=90, size in 4_u8..=20, rise in -5_i8..=5,
    ) {
        let (first,second) = if reverse { ("G","F") } else { ("F","G") };
        let parsed = document(2,&format!("BT /{first} {size} Tf 1 0 0 1 {x} {y} Tm {rise} Ts <0041> Tj /{second} {size} Tf <0042> Tj ET"),in_form);
        prop_assert_eq!(parsed.text_elements.len(),2);
        prop_assert_eq!(parsed.text_elements[0].text.as_str(),"A");
        prop_assert_eq!(parsed.text_elements[1].text.as_str(),"B");
        let expected = (f32::from(y)+f32::from(rise))*if in_form { 3. } else { 1. }+if in_form { 11. } else { 0. };
        let known = usize::from(reverse);
        prop_assert_eq!(parsed.text_elements[known].baseline_y,Some(expected));
        prop_assert_eq!(parsed.text_elements[1-known].baseline_y,None);
    }
}

/// 埋め込み水平CMapのCID経路も、名前から推測せず対応済みmetricsで基線を確認する。
#[test]
fn embedded_horizontal_cmap_keeps_verified_source_baseline() {
    assert_baselines(6);
}
