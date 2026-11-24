//! 確認済みbaselineの小さい文字を、元のfont sizeのまま上下へ配置する回帰。
use super::*;

/// 実PDFを保存せず、上付きの数字と独立した次の本文行を持つ解析結果を作る。
fn formula_source() -> ParsedDocument {
    let mut parsed = source(2, &[]);
    parsed.page_count = 1;
    parsed.pages.truncate(1);
    parsed.text_elements = [
        ("A", 50., 56., 700., 10.),
        ("2", 56.2, 60.2, 704., 7.),
        ("=B", 62., 76., 700., 10.),
        ("Next line", 50., 100., 680., 10.),
    ]
    .into_iter()
    .map(|(text, left, right, baseline, size)| ParsedTextElement {
        baseline_y: Some(baseline),
        page: 0,
        bounds: ParsedBounds {
            x0: left,
            x1: right,
            y0: baseline - size * 0.2,
            y1: baseline + size * 0.8,
        },
        font_size: size,
        font_name: Some("Helvetica".into()),
        fill_color: None,
        text: text.into(),
    })
    .collect();
    parsed
}

/// 本文の行を増やさず、元のfont sizeを保ったまま上付き位置を出力する。
#[test]
fn raised_source_glyph_stays_with_its_base_line() {
    let parsed = formula_source();
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(pages.len(), 1);
    assert_eq!(
        pages[0].lines.len(),
        2,
        "raised source glyph must stay with its base line"
    );
    assert_eq!(pages[0].lines[0].text, "A2=B");
    assert_eq!(pages[0].lines[1].text, "Next line");
    let (xml, _) = package_source(parsed);
    assert!(
        xml.contains("<w:position w:val=\"8\""),
        "source4pt baseline offset requires8halfpoints without font shrink"
    );
    assert!(xml.contains("<w:sz w:val=\"14\""));
    assert!(!xml.contains("<w:vertAlign"));
    assert!(!xml.contains("JadeBaseline_"));
}

/// ページを半分へ縮小すると、font sizeとbaseline位置も同じ物理倍率で縮小する。
#[test]
fn inline_offsets_follow_the_existing_word_page_fit() {
    let mut parsed = formula_source();
    parsed.pages[0].width = 3168.;
    let (xml, _) = package_source(parsed);
    assert!(xml.contains("<w:position w:val=\"4\""));
    assert!(xml.contains("<w:sz w:val=\"7\""));
    assert!(!xml.contains("JadeBaseline_"));
}

/// 元baselineが不明、または通常文字の基準が一意でなければ上付き位置を推測しない。
#[test]
fn unknown_or_ambiguous_baselines_keep_existing_run_formatting() {
    for ambiguous in [false, true] {
        let mut parsed = formula_source();
        if ambiguous {
            parsed.text_elements[2].baseline_y = Some(701.);
        } else {
            parsed.text_elements[1].baseline_y = None;
        }
        let (xml, _) = package_source(parsed);
        assert!(!xml.contains("<w:position"));
        assert!(!xml.contains("JadeBaseline_"));
    }
}

/// 離れた数式記号の未知baselineが、通常数字に囲まれた指数の既知位置を失わせない。
#[test]
fn distant_unknown_formula_glyph_does_not_flatten_verified_exponent() {
    let mut parsed = formula_source();
    let mut unknown = parsed.text_elements[0].clone();
    unknown.text = "π".into();
    unknown.bounds.x0 = 5.;
    unknown.bounds.x1 = 11.;
    unknown.bounds.y0 = 700.;
    unknown.bounds.y1 = 710.;
    unknown.baseline_y = None;
    parsed.text_elements.push(unknown);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(pages[0].lines.len(), 2);
    assert!(pages[0].lines[0].text.contains('π'));
    let (xml, _) = package_source(parsed);
    assert!(
        xml.contains("<w:position w:val=\"8\""),
        "nearby verified main glyphs must retain the exponent despite a distant unknown baseline"
    );
    assert!(xml.contains("π"));
    assert!(!xml.contains("JadeBaseline_"));
}

/// 指数に隣接する通常文字の基線が不明なら、遠い正常な文字から補完しない。
#[test]
fn adjacent_unknown_anchor_still_blocks_a_guessed_offset() {
    let mut parsed = formula_source();
    let mut unknown = parsed.text_elements[0].clone();
    unknown.text = "π".into();
    unknown.bounds.x0 = 60.5;
    unknown.bounds.x1 = 61.5;
    unknown.bounds.y0 = 700.;
    unknown.bounds.y1 = 710.;
    unknown.baseline_y = None;
    parsed.text_elements.push(unknown);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(pages[0].lines.len(), 2);
    assert!(pages[0].lines[0].text.contains('π'));
    let (xml, _) = package_source(parsed);
    assert!(!xml.contains("<w:position"));
    assert!(xml.contains("π"));
}

/// ファイルAPIの一時ZIPでも、privateなstyle名を公開せずbytesと同じ位置へ変換する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn file_output_compiles_the_same_source_offset_as_bytes() {
    use std::io::Read;
    let mut parsed = formula_source();
    let docx = build_structured_docx_from_owned_images(&mut parsed, &None, &[]).unwrap();
    let file = package::pack_docx_to_file(docx, &[]).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(
        !xml.contains("JadeBaseline_"),
        "file output must compile baseline markers"
    );
    assert!(xml.contains("<w:position w:val=\"8\""));
    let (bytes_xml, _) = package_source(formula_source());
    assert_eq!(xml, bytes_xml);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 原点の平行移動や上下の符号によらず、文字順とfont sizeを保ち相対half-point位置を残す。
    #[test]
    fn signed_offsets_are_invariant_under_source_translation(
        x in 10i16..300, y in 100i16..700,
        half_points in (-8i32..=8).prop_filter("nonzero", |value| *value != 0),
    ) {
        let mut parsed = formula_source();
        for text in &mut parsed.text_elements {
            let dx = f32::from(x) - 50.;
            let dy = f32::from(y) - 700.;
            text.bounds.x0 += dx;
            text.bounds.x1 += dx;
            text.bounds.y0 += dy;
            text.bounds.y1 += dy;
            text.baseline_y = text.baseline_y.map(|base| base + dy);
        }
        let delta = half_points as f32 / 2. - 4.;
        let script = &mut parsed.text_elements[1];
        script.baseline_y = script.baseline_y.map(|base| base + delta);
        script.bounds.y0 += delta;
        script.bounds.y1 += delta;
        let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
        prop_assert_eq!(pages[0].lines.len(), 2);
        prop_assert_eq!(&pages[0].lines[0].text, "A2=B");
        let (xml, _) = package_source(parsed);
        let expected = format!("<w:position w:val=\"{half_points}\"");
        prop_assert!(xml.contains(&expected));
        prop_assert!(xml.contains("<w:sz w:val=\"14\""));
        prop_assert!(!xml.contains("JadeBaseline_"));
    }

    /// 他の式の未知記号が増えても、平行移動した局所の数値と指数の位置は変わらない。
    #[test]
    fn nonlocal_unknown_symbols_preserve_translated_signed_offsets(
        count in 1usize..20, x in 0i16..200, y in 100i16..700,
        half_points in (-8i32..=8).prop_filter("nonzero", |value| *value != 0),
    ) {
        let mut parsed = formula_source();
        let delta = half_points as f32 / 2. - 4.;
        let script = &mut parsed.text_elements[1];
        script.baseline_y = script.baseline_y.map(|base| base + delta);
        script.bounds.y0 += delta;
        script.bounds.y1 += delta;
        for index in 0..count {
            let mut unknown = parsed.text_elements[0].clone();
            unknown.text = "π".into();
            unknown.bounds.x0 = 1. + index as f32 * 1.5;
            unknown.bounds.x1 = unknown.bounds.x0 + 1.;
            unknown.bounds.y0 = 700.;
            unknown.bounds.y1 = 710.;
            unknown.baseline_y = None;
            parsed.text_elements.push(unknown);
        }
        for text in &mut parsed.text_elements {
            let dx = f32::from(x);
            let dy = f32::from(y) - 700.;
            text.bounds.x0 += dx;
            text.bounds.x1 += dx;
            text.bounds.y0 += dy;
            text.bounds.y1 += dy;
            text.baseline_y = text.baseline_y.map(|base| base + dy);
        }
        let (xml, _) = package_source(parsed);
        let expected = format!("<w:position w:val=\"{half_points}\"");
        prop_assert!(xml.contains(&expected));
        prop_assert_eq!(xml.matches('π').count(), count);
        prop_assert!(xml.contains("<w:sz w:val=\"14\""));
        prop_assert!(!xml.contains("JadeBaseline_"));
    }
}
