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
        source_background: None,
        fill_stroke_width: None,
        font_weight: None,
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

/// 固定4ptの行閾値を超える指数も、確認済みの隣接本文と同じ行へ保持する。
#[test]
fn raised_script_beyond_fixed_tolerance_retains_numeric_row() {
    let mut parsed = formula_source();
    let script = &mut parsed.text_elements[1];
    script.baseline_y = Some(704.5);
    script.bounds.y0 += 0.5;
    script.bounds.y1 += 0.5;
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(
        pages[0].lines.len(),
        2,
        "verified4.5pt source exponent must remain in its numeric row"
    );
    assert_eq!(pages[0].lines[0].text, "A2=B");
    let (xml, _) = package_source(parsed);
    assert!(xml.contains("<w:position w:val=\"9\""));
}

/// 直下の小さい本文行は、上の本文と水平に重なるので添字として吸収しない。
#[test]
fn nearby_small_text_line_remains_separate() {
    let mut parsed = formula_source();
    let mut note = parsed.text_elements[1].clone();
    note.text = "note".into();
    note.bounds.x0 = 50.;
    note.bounds.x1 = 76.;
    note.bounds.y0 = 693.1;
    note.bounds.y1 = 700.1;
    note.baseline_y = Some(694.5);
    parsed.text_elements.push(note);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(pages[0].lines.len(), 3);
    assert_eq!(pages[0].lines[1].text, "note");
}

/// 基線不明の離れた文字は、矩形だけから数値行の指数へ変更しない。
#[test]
fn unverified_outlying_script_remains_separate() {
    let mut parsed = formula_source();
    let script = &mut parsed.text_elements[1];
    script.baseline_y = None;
    script.bounds.y0 = 704.5;
    script.bounds.y1 = 711.5;
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(pages[0].lines.len(), 3);
    let (xml, _) = package_source(parsed);
    assert!(!xml.contains("<w:position"));
}

/// 直前の細い文字の後ろに長い本文boxが重なる場合も、その交差を見落とさない。
#[test]
fn overlapping_main_span_blocks_outlying_row_recovery() {
    let mut parsed = formula_source();
    parsed.text_elements[1].baseline_y = Some(704.5);
    parsed.text_elements[1].bounds.y0 += 0.5;
    parsed.text_elements[1].bounds.y1 += 0.5;
    for (text, left, right) in [("wide", 49., 61.), ("i", 54., 55.)] {
        let mut main = parsed.text_elements[0].clone();
        main.text = text.into();
        main.bounds.x0 = left;
        main.bounds.x1 = right;
        parsed.text_elements.push(main);
    }
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(
        pages[0].lines.len(),
        3,
        "overlapping main text must keep the outlying row separate"
    );
}

/// 上下二つの本文に同じように接する場合は、行の対応を推測しない。
#[test]
fn two_valid_body_rows_leave_outlying_script_ambiguous() {
    let mut parsed = formula_source();
    parsed.text_elements[1].baseline_y = Some(694.5);
    parsed.text_elements[1].bounds.y0 -= 9.5;
    parsed.text_elements[1].bounds.y1 -= 9.5;
    for index in [0, 2] {
        let mut lower = parsed.text_elements[index].clone();
        lower.baseline_y = Some(689.);
        lower.bounds.y0 -= 11.;
        lower.bounds.y1 -= 11.;
        parsed.text_elements.push(lower);
    }
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(pages[0].lines.len(), 4);
}

/// 離れた不明な添字clusterを残しても、既知の数値clusterの行対応は失わない。
#[test]
fn independent_script_cluster_recovers_despite_unknown_neighbour_elsewhere() {
    let mut parsed = formula_source();
    parsed.text_elements[1].baseline_y = Some(704.5);
    parsed.text_elements[1].bounds.y0 += 0.5;
    parsed.text_elements[1].bounds.y1 += 0.5;
    let mut orphan = parsed.text_elements[1].clone();
    orphan.text = "3".into();
    orphan.bounds.x0 = 10.;
    orphan.bounds.x1 = 14.;
    parsed.text_elements.push(orphan);
    let mut unknown = parsed.text_elements[0].clone();
    unknown.text = "π".into();
    unknown.bounds.x0 = 5.;
    unknown.bounds.x1 = 10.;
    unknown.bounds.y0 = 700.;
    unknown.bounds.y1 = 710.;
    unknown.baseline_y = None;
    parsed.text_elements.push(unknown);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    let body = pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains('A'))
        .unwrap();
    assert!(
        body.text.ends_with("A2=B"),
        "verified numeric cluster must recover independently of an unresolved script cluster"
    );
    assert_eq!(pages[0].lines.len(), 3);
    let (xml, _) = package_source(parsed);
    assert!(xml.contains("<w:position w:val=\"9\""));
    assert!(xml.contains('3'));
}

/// 添字で縦範囲が4ptを超えても、通常単語間の実source空白を捨てない。
#[test]
fn recovered_script_row_preserves_main_word_space() {
    let mut parsed = formula_source();
    parsed.text_elements[0].text = "Alpha".into();
    parsed.text_elements[0].bounds.x1 = 80.;
    parsed.text_elements[1].baseline_y = Some(704.5);
    parsed.text_elements[1].bounds.x0 = 108.2;
    parsed.text_elements[1].bounds.x1 = 112.2;
    parsed.text_elements[1].bounds.y0 += 0.5;
    parsed.text_elements[1].bounds.y1 += 0.5;
    parsed.text_elements[2].text = "Beta".into();
    parsed.text_elements[2].bounds.x0 = 84.;
    parsed.text_elements[2].bounds.x1 = 108.;
    let mut end = parsed.text_elements[2].clone();
    end.text = "=B".into();
    end.bounds.x0 = 114.;
    end.bounds.x1 = 128.;
    let mut space = end.clone();
    space.text = " ".into();
    space.bounds.x0 = 80.;
    space.bounds.x1 = 84.;
    parsed.text_elements.extend([end, space]);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(
        pages[0].lines[0].text, "Alpha Beta2=B",
        "source word space must survive script row recovery"
    );
}

/// 上付きの語同士の空白も、通常本文の基線へ移さず元の高さへ保持する。
#[test]
fn recovered_script_words_preserve_raised_space() {
    let mut parsed = formula_source();
    parsed.text_elements[1].text = "x".into();
    parsed.text_elements[1].baseline_y = Some(704.5);
    parsed.text_elements[1].bounds.y0 += 0.5;
    parsed.text_elements[1].bounds.y1 += 0.5;
    parsed.text_elements[2].text = "B".into();
    parsed.text_elements[2].bounds.x0 = 68.;
    parsed.text_elements[2].bounds.x1 = 74.;
    let mut right = parsed.text_elements[1].clone();
    right.text = "y".into();
    right.bounds.x0 = 62.;
    right.bounds.x1 = 66.;
    let mut space = right.clone();
    space.text = " ".into();
    space.bounds.x0 = 60.2;
    space.bounds.x1 = 62.;
    parsed.text_elements.extend([right, space]);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(
        pages[0].lines[0].text, "Ax yB",
        "raised source word space must retain its baseline"
    );
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
    /// 固定閾値を越える上下の添字でも、原点に依存せず行・文字順・元fontを保つ。
    #[test]
    fn outlying_scripts_recover_under_translation_and_both_signs(
        half_points in 9i32..=12, raised in any::<bool>(),
        x in 0i16..200, y in 100i16..700,
    ) {
        let mut parsed = formula_source();
        let signed = if raised { half_points } else { -half_points };
        let delta = signed as f32 / 2. - 4.;
        parsed.text_elements[1].baseline_y = Some(700. + signed as f32 / 2.);
        parsed.text_elements[1].bounds.y0 += delta;
        parsed.text_elements[1].bounds.y1 += delta;
        for text in &mut parsed.text_elements {
            let dx = f32::from(x);
            let dy = f32::from(y) - 700.;
            text.bounds.x0 += dx;
            text.bounds.x1 += dx;
            text.bounds.y0 += dy;
            text.bounds.y1 += dy;
            text.baseline_y = text.baseline_y.map(|base| base + dy);
        }
        let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
        prop_assert_eq!(pages[0].lines.len(), 2);
        prop_assert_eq!(&pages[0].lines[0].text, "A2=B");
        let (xml, _) = package_source(parsed);
        let expected = format!("<w:position w:val=\"{signed}\"");
        prop_assert!(xml.contains(&expected));
        prop_assert!(xml.contains("<w:sz w:val=\"14\""));
        prop_assert!(!xml.contains("JadeBaseline_"));
    }

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

/// 任意のfragment粒度で、通常本文の近くに独立した小文字の長い行を作る。
fn diagonal_note_source(split_glyphs: bool, gap: f32, offset: f32) -> ParsedDocument {
    let mut parsed = formula_source();
    let note = "an independent footnote paragraph";
    let mut fragment = parsed.text_elements[1].clone();
    fragment.baseline_y = Some(700. + offset);
    fragment.bounds.y0 = 698.6 + offset;
    fragment.bounds.y1 = 705.6 + offset;
    fragment.bounds.x0 = 76. + gap;
    fragment.bounds.x1 = fragment.bounds.x0 + note.len() as f32 * 3.5;
    fragment.text = note.into();
    if split_glyphs {
        for (i, c) in note.chars().enumerate() {
            let mut glyph = fragment.clone();
            glyph.text = c.to_string();
            glyph.bounds.x0 = fragment.bounds.x0 + i as f32 * 3.5;
            glyph.bounds.x1 = glyph.bounds.x0 + 3.5;
            parsed.text_elements.push(glyph);
        }
    } else {
        parsed.text_elements.push(fragment);
    }
    parsed
}

/// 段の根拠がない斜め下の長い小文字行を、span分割の有無に関係なく本文へ吸収しない。
#[test]
fn long_diagonal_footnote_is_not_an_inline_script() {
    for split_glyphs in [false, true] {
        let parsed = diagonal_note_source(split_glyphs, 4., -5.5);
        let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
        let body = pages[0]
            .lines
            .iter()
            .find(|l| l.text.starts_with('A'))
            .unwrap();
        assert_eq!(
            body.text, "A2=B",
            "diagonal footnote must not become an inline script (split={split_glyphs})"
        );
        assert!(
            pages[0]
                .lines
                .iter()
                .any(|l| l.text == "an independent footnote paragraph")
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 行の幅と位置・描画順・fragment粒度によらず、脚注本文と数式の文字を混ぜない。
    #[test]
    fn diagonal_footnote_identity_is_representation_invariant(
        split in any::<bool>(), reverse in any::<bool>(), above in any::<bool>(),
        gap in 0_u8..=20, half_points in 9_u8..=12, translate in -50_i32..=20,
    ) {
        let delta = f32::from(half_points) / 2. * if above { 1. } else { -1. };
        let mut parsed = diagonal_note_source(split, f32::from(gap), delta);
        for t in &mut parsed.text_elements {
            t.baseline_y = t.baseline_y.map(|b| b + translate as f32);
            t.bounds.y0 += translate as f32;
            t.bounds.y1 += translate as f32;
        }
        if reverse { parsed.text_elements.reverse(); }
        let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
        prop_assert!(pages[0].lines.iter().any(|l| l.text == "A2=B"));
        prop_assert!(pages[0].lines.iter().any(|l| l.text == "an independent footnote paragraph"));
        prop_assert_eq!(pages[0].lines.len(), 3);
    }
}
