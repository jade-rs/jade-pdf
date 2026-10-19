//! Identity-H の CID 幅と、Unicode 文字数によらない文字原点の回帰。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 一つのencoded glyphが複数Unicodeへ対応しても、clipとadvanceはcode単位で扱う。
#[test]
fn partial_clip_preserves_variable_code_and_ligature_boundaries() {
    for in_form in [false, true] {
        let parsed = parse_with_font(
            "<< /Type /Font /Subtype /Type0 /Encoding 13 0 R /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>",
            "<< /Subtype /CIDFontType2 /W [2 [700 200] 50 [300]] >>",
            "q 16 5 10 20 re W n BT /F 10 Tf 1 Tc 5 Tw 10 10 Td <20810021> Tj 1 0 0 rg <21> Tj ET Q",
            in_form,
            "2 begincodespacerange\n<00> <7F>\n<8100> <81FF>\nendcodespacerange\n3 beginbfchar\n<20> <00660069>\n<8100> <0042>\n<21> <0043>\nendbfchar",
            &[stream(
                "/WMode 0 def 2 begincodespacerange <00> <7F> <8100> <81FF> endcodespacerange 3 begincidchar <20> 2 <8100> 50 <21> 3 endcidchar",
            )],
        );
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|text| text.text.as_str())
                .collect::<String>(),
            "fiB"
        );
        assert_eq!(
            [
                parsed.text_elements[0].bounds.x0,
                parsed.text_elements[0].bounds.x1
            ],
            [16., 26.]
        );
        // 次のTjは元operand全体のadvanceを使い、clip外のため本文には現れない。
        assert_eq!(parsed.text_elements.len(), 1);
    }
}

/// 可変長 Encoding は Unicode の文字数とは独立に CID を選び、一 byte の32だけに Tw を適用する。
#[test]
fn variable_encoding_cmap_selects_cids_and_source_word_spacing() {
    for in_form in [false, true] {
        let parsed = parse_with_font(
            "<< /Type /Font /Subtype /Type0 /Encoding 13 0 R /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>",
            "<< /Subtype /CIDFontType2 /W [2 [700 200] 50 [300]] >>",
            "BT /F 10 Tf 1 Tc 5 Tw 10 10 Td <20810021> Tj /F 20 Tf <21> Tj ET",
            in_form,
            "2 begincodespacerange\n<00> <7F>\n<8100> <81FF>\nendcodespacerange\n3 beginbfchar\n<20> <00660069>\n<8100> <0042>\n<21> <0043>\nendbfchar",
            &[stream(
                "/WMode 0 def 2 begincodespacerange <00> <7F> <8100> <81FF> endcodespacerange 3 begincidchar <20> 2 <8100> 50 <21> 3 endcidchar",
            )],
        );
        assert_eq!(parsed.text_elements[0].text, "fiBC");
        assert_eq!(parsed.text_elements[0].bounds.x1, 29.);
        assert_eq!(parsed.text_elements[1].bounds.x0, 30.);
        assert_eq!(parsed.text_elements[1].bounds.x1, 34.);
    }
}

/// 三・四 byte の codespace と cidrange を組み合わせ、範囲外 CID には DW を使う。
#[test]
fn variable_encoding_cidrange_supports_three_and_four_byte_codes() {
    for in_form in [false, true] {
        let parsed = parse_with_font(
            "<< /Type /Font /Subtype /Type0 /Encoding 13 0 R /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>",
            "<< /Subtype /CIDFontType2 /W [100 [800]] /DW 300 >>",
            "BT /F 10 Tf 10 10 Td <900001900002A0000001> Tj /F 20 Tf <900001> Tj ET",
            in_form,
            "2 begincodespacerange\n<900000> <90FFFF>\n<A0000000> <A0FFFFFF>\nendcodespacerange\n3 beginbfchar\n<900001> <0041>\n<900002> <0042>\n<A0000001> <0043>\nendbfchar",
            &[stream(
                "2 begincodespacerange <900000> <90FFFF> <A0000000> <A0FFFFFF> endcodespacerange 1 begincidrange <900001> <900002> 100 endcidrange 1 begincidchar <A0000001> 200 endcidchar",
            )],
        );
        assert_eq!(parsed.text_elements[0].text, "ABC");
        assert_eq!(parsed.text_elements[0].bounds.x1, 24.);
        assert_eq!(parsed.text_elements[1].bounds.x0, 24.);
        assert_eq!(parsed.text_elements[1].bounds.x1, 40.);
    }
}

/// 指定した descendant と補助 object を持つ Type0 Font をページ/Form に描画する。
fn parse(descendant: &str, content: &str, in_form: bool, extra: &[String]) -> ParsedDocument {
    parse_with_font(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Custom /Encoding /Identity-H /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>",
        descendant,
        content,
        in_form,
        "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n4 beginbfchar\n<0020> <0020>\n<0041> <0041>\n<0042> <0042>\n<0043> <0043>\nendbfchar",
        extra,
    )
}

/// Encoding・descendant 参照・ToUnicode を独立に設定した Font で PDF を生成する。
fn parse_with_font(
    font: &str,
    descendant: &str,
    content: &str,
    in_form: bool,
    cmap: &str,
    extra: &[String],
) -> ParsedDocument {
    let mut objects = pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = stream(if in_form { "/Fm Do" } else { content });
    objects[5] = stream("");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push(font.into());
    objects.push(descendant.into());
    objects.push(stream(cmap));
    objects.extend_from_slice(extra);
    parse_pdf_bytes("cid-widths.pdf", &pdf(&objects)).unwrap()
}

/// 一対多 ToUnicode の glyph も幅・Tc は一つの CID として数える。
#[test]
fn unicode_ligature_keeps_one_cid_advance() {
    let parsed = parse_with_font(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Custom /Encoding /Identity-H /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>",
        "<< /Type /Font /Subtype /CIDFontType2 /W [65 [700]] >>",
        "BT /F 10 Tf 2 Tc 10 10 Td <0041> Tj /F 20 Tf <0042> Tj ET",
        true,
        "2 beginbfchar\n<0041> <00660069>\n<0042> <0042>\nendbfchar",
        &[],
    );
    assert_eq!(parsed.text_elements[0].text, "fi");
    assert_eq!(parsed.text_elements[0].bounds.x1, 17.);
    assert_eq!(parsed.text_elements[1].bounds.x0, 19.);
}

/// DescendantFonts 配列と Encoding/descendant Subtype の間接参照を解決する。
#[test]
fn indirect_encoding_and_descendant_array_are_resolved() {
    let parsed = parse_with_font(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Custom /Encoding 13 0 R /DescendantFonts 14 0 R /ToUnicode 12 0 R >>",
        "<< /Type /Font /Subtype 15 0 R /W [65 [900]] >>",
        "BT /F 10 Tf 10 10 Td <0041> Tj ET",
        true,
        "1 beginbfchar\n<0041> <0041>\nendbfchar",
        &[
            "/Identity-H".into(),
            "[11 0 R]".into(),
            "/CIDFontType2".into(),
        ],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 19.);
}

/// Type0 の Subtype が間接値でも水平 CID 幅を採用する。
#[test]
fn indirect_type0_subtype_keeps_cid_metrics() {
    let parsed = parse_with_font(
        "<< /Type /Font /Subtype 13 0 R /BaseFont /Custom /Encoding /Identity-H /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>",
        "<< /Type /Font /Subtype /CIDFontType2 /W [65 [900]] >>",
        "BT /F 10 Tf 10 10 Td <0041> Tj ET",
        true,
        "1 beginbfchar\n<0041> <0041>\nendbfchar",
        &["/Type0".into()],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 19.);
}

/// CIDFontType0/2 の個別幅を raw CID に対応付け、後続描画にも適用する。
#[test]
fn individual_cid_widths_apply_on_pages_and_forms() {
    for subtype in ["CIDFontType0", "CIDFontType2"] {
        for in_form in [false, true] {
            let parsed = parse(
                &format!("<< /Type /Font /Subtype /{subtype} /W [65 [900 300]] >>"),
                "BT /F 10 Tf 10 10 Td <00410042> Tj /F 20 Tf <0041> Tj ET",
                in_form,
                &[],
            );
            assert_eq!(parsed.text_elements[0].text, "AB");
            assert_eq!(parsed.text_elements[0].bounds.x1, 22.);
            assert_eq!(parsed.text_elements[1].bounds.x0, 22.);
            assert_eq!(parsed.text_elements[1].bounds.x1, 40.);
        }
    }
}

/// 範囲形式の W と範囲外の DW を同じ文字列内で使う。
#[test]
fn range_widths_and_default_width_are_combined() {
    let parsed = parse(
        "<< /Type /Font /Subtype /CIDFontType2 /W [65 66 700] /DW 500 >>",
        "BT /F 10 Tf 10 10 Td <004100430042> Tj /F 20 Tf <0043> Tj ET",
        true,
        &[],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 29.);
    assert_eq!(parsed.text_elements[1].bounds.x0, 29.);
    assert_eq!(parsed.text_elements[1].bounds.x1, 39.);
}

/// DW 省略時は1000 units/em を使い、Unicode 用の推定幅へ戻さない。
#[test]
fn omitted_default_width_is_one_em() {
    let parsed = parse(
        "<< /Type /Font /Subtype /CIDFontType2 >>",
        "BT /F 10 Tf 10 10 Td <00410042> Tj /F 20 Tf <0043> Tj ET",
        true,
        &[],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 30.);
    assert_eq!(parsed.text_elements[1].bounds.x0, 30.);
    assert_eq!(parsed.text_elements[1].bounds.x1, 50.);
}

/// W 配列・CID・幅・DW の間接値を解決する。
#[test]
fn indirect_cid_width_entries_are_resolved() {
    let parsed = parse(
        "<< /Type /Font /Subtype /CIDFontType2 /W 13 0 R /DW 18 0 R >>",
        "BT /F 10 Tf 10 10 Td <00410043> Tj ET",
        true,
        &[
            "[14 0 R 15 0 R]".into(),
            "65".into(),
            "[16 0 R]".into(),
            "17 0 R".into(),
            "800".into(),
            "250".into(),
        ],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 20.5);
}

/// Identity-H の0020は2 byte code なので Tw を適用せず、Tc は各 CID へ適用する。
#[test]
fn identity_space_uses_character_spacing_without_word_spacing() {
    let parsed = parse(
        "<< /Type /Font /Subtype /CIDFontType2 >>",
        "BT /F 10 Tf 2 Tc 50 Tw 50 Tz 10 10 Td <00410020> Tj /F 20 Tf <0041> Tj ET",
        true,
        &[],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 21.);
    assert_eq!(parsed.text_elements[1].bounds.x0, 22.);
    assert_eq!(parsed.text_elements[1].bounds.x1, 32.);
}

/// Literal と hex の両入力を同じ2 byte code として処理する。
#[test]
fn literal_octal_and_hex_have_the_same_cid_width() {
    for raw in ["<00410042>", "(\\000A\\000B)"] {
        let parsed = parse(
            "<< /Type /Font /Subtype /CIDFontType2 /W [65 [800 200]] >>",
            &format!("BT /F 10 Tf 1 Tc 10 10 Td {raw} Tj /F 20 Tf <0043> Tj ET"),
            true,
            &[],
        );
        assert_eq!(parsed.text_elements[0].bounds.x1, 21.);
        assert_eq!(parsed.text_elements[1].bounds.x0, 22.);
    }
}

/// TJ の数値調整は CID の幅・Tc を適用した原点から移動する。
#[test]
fn tj_adjustment_uses_cid_advance() {
    let parsed = parse(
        "<< /Type /Font /Subtype /CIDFontType2 /W [65 [900 300]] >>",
        "BT /F 10 Tf 1 Tc 50 Tz 10 10 Td [<0041> 200 <0042>] TJ /F 20 Tf <0043> Tj ET",
        true,
        &[],
    );
    assert_eq!(parsed.text_elements[0].bounds.x1, 15.5);
    assert_eq!(parsed.text_elements[1].bounds.x0, 16.);
}

/// 負の advance で後続原点がページ外になれば glyph bounds は境界へ clip する。
#[test]
fn split_negative_advance_clips_following_glyph_at_page_edge() {
    let descendant = "<< /Type /Font /Subtype /CIDFontType2 /W [65 [100]] >>";
    for show in [
        format!("<{}> Tj", "0041".repeat(11)),
        "<0041> Tj ".repeat(11),
        format!("[{}] TJ", "<0041> ".repeat(11)),
    ] {
        let parsed = parse(
            descendant,
            &format!("BT /F 10 Tf -2 Tc 91 Tz 10 10 Td {show} /F 20 Tf <0043> Tj ET"),
            true,
            &[],
        );
        let glyph = parsed.text_elements.last().unwrap();
        assert_eq!(glyph.text, "C");
        assert_eq!(glyph.bounds.x0, 0.);
        assert!((glyph.bounds.x1 - 18.19).abs() < 0.001);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
    /// clip のないページ内で Tj/TJ 分割にかかわらず、CID 幅・Tc・横倍率の最終原点が一致する。
    #[test]
    fn splitting_preserves_cid_advance(count in 1usize..12, width in 100u16..1000, spacing in -2i16..5, scale in 25u16..150) {
        let descendant = format!("<< /Type /Font /Subtype /CIDFontType2 /W [65 [{}]] >>", width);
        let prefix = format!("BT /F 10 Tf {spacing} Tc {scale} Tz 100 10 Td");
        let whole = parse(&descendant, &format!("{prefix} <{}> Tj /F 20 Tf <0043> Tj ET", "0041".repeat(count)), true, &[]);
        let expected = 100. + count as f32 * (width as f32 / 100. + spacing as f32) * scale as f32 / 100.;
        for show in ["<0041> Tj ".repeat(count), format!("[{}] TJ", "<0041> ".repeat(count))] {
            let split = parse(&descendant, &format!("{prefix} {show} /F 20 Tf <0043> Tj ET"), true, &[]);
            let origin = split.text_elements.last().unwrap().bounds.x0;
            prop_assert!((origin - expected).abs() < 0.001, "actual {origin}, expected {expected}");
            prop_assert!((origin - whole.text_elements.last().unwrap().bounds.x0).abs() < 0.001);
        }
    }
}

/// 間接 Font の別名と二ページで Encoding 表を共有し、予算超過は公開エラーとして返す。
#[test]
fn encoding_allocation_is_budgeted_and_shared_by_font_aliases() {
    use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes_with_limits};
    let mut objects =
        pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R /Alias 10 0 R >> >>");
    objects[4] = stream("BT /F 10 Tf 10 10 Td (A) Tj ET");
    objects[5] = stream("BT /Alias 10 Tf 10 10 Td (A) Tj ET");
    objects.push(
        "<< /Type /Font /Subtype /Type0 /Encoding 12 0 R /DescendantFonts [11 0 R] >>".into(),
    );
    objects.push("<< /Subtype /CIDFontType2 /DW 900 >>".into());
    objects.push(stream(
        "1 begincodespacerange <00> <FF> endcodespacerange 1 begincidchar <41> 100 endcidchar",
    ));
    let bytes = pdf(&objects);
    let error = parse_pdf_bytes_with_limits(
        "encoding-budget.pdf",
        &bytes,
        &ParseLimits {
            max_total_font_metrics_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap_err();
    let ParseError::ResourceLimit {
        resource: "font metrics bytes",
        actual: capacity,
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert!(capacity > 0);
    let parsed = parse_pdf_bytes_with_limits(
        "encoding-budget.pdf",
        &bytes,
        &ParseLimits {
            max_total_font_metrics_bytes: capacity,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.text_elements.len(), 2);
    for element in parsed.text_elements {
        assert_eq!(element.bounds.x1, 19.);
    }
}
