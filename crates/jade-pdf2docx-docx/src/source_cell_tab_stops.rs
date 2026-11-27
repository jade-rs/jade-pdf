//! 閉じたセルの一行にある明確な空隙を、文字群の原点を持つWordタブへ渡す。
use super::*;

/// 本文・font・幾何を全断片で確認し、大きな空隙がある行だけ局所位置を返す。
pub(super) fn positions(
    row: &[&ParsedTextElement],
    line: &StructuredTextLine,
    left: f32,
    right: f32,
    fonts: &[ParsedEmbeddedFont],
) -> Option<Vec<Option<usize>>> {
    if row.len() < 2
        || row.len() != line.runs.len()
        || !left.is_finite()
        || !right.is_finite()
        || left >= right
    {
        return None;
    }
    let mut output = vec![None; row.len()];
    let mut large_gap = false;
    for (index, (text, run)) in row.iter().zip(&line.runs).enumerate() {
        let b = text.bounds;
        let style = &run.style;
        let embedded = style.font_embedded && style.embedded_face.is_some();
        let known_ascii = style.font_name.as_deref() == Some("Helvetica")
            && !style.font_embedded
            && style.fill_stroke_width.is_none()
            && style.font_weight.is_none_or(|weight| weight <= 500)
            && (style.font_size * 2.).fract() == 0.
            && text.text.bytes().all(|c| (b' '..=b'~').contains(&c));
        if (!embedded && !known_ascii)
            || style.placement.is_some()
            || ![b.x0, b.x1, b.y0, b.y1, style.font_size]
                .iter()
                .all(|v| v.is_finite())
            || style.font_size <= 0.
            || b.x0 < left
            || b.x1 > right
            || b.x0 >= b.x1
            || b.y0 >= b.y1
            || text.text.trim().is_empty()
            || text.text.trim() != run.text.trim()
            || !text.text.chars().all(|c| {
                !c.is_control()
                    && (c.is_ascii()
                        || ('\u{3000}'..='\u{9fff}').contains(&c)
                        || ('\u{ff01}'..='\u{ffef}').contains(&c))
            })
        {
            return None;
        }
        if index == 0 {
            continue;
        }
        let prior = row[index - 1];
        let gap = b.x0 - prior.bounds.x1;
        if text.page != prior.page || gap < -0.001 {
            return None;
        }
        large_gap |= gap > style.font_size.max(prior.font_size) * 2.;
        if gap > 0.5 {
            let pos = pdf_points_to_twips(b.x0 - left)?;
            if pos == 0 {
                return None;
            }
            output[index] = Some(usize::try_from(pos).ok()?);
        }
    }
    if !large_gap {
        return None;
    }
    let mut cursor = f64::from(row[0].bounds.x0);
    for ((text, run), position) in row.iter().zip(&line.runs).zip(&output) {
        if let Some(pos) = position {
            let target = f64::from(left) + *pos as f64 / 20.;
            // 次のstopを既に越えたWord runにタブを渡すと、既定stopや次行へ飛んでしまう。
            if cursor + 0.05 >= target {
                return None;
            }
            cursor = target;
        }
        let value = if position.is_some() {
            run.text.trim_start()
        } else {
            &run.text
        };
        let width = if run.style.font_embedded {
            let font = fonts
                .iter()
                .find(|font| Some(font.font_name.as_str()) == run.style.font_name.as_deref())?;
            let half = pdf_font_size_to_half_points(run.style.font_size)?;
            simple_advance(&font.bytes, value)? * half as f64 / 2.
        } else {
            // 既知Helvetica ASCIIは原本幅に加え、一文字一emの保守的な上限でもstopまで収まる必要がある。
            f64::from(text.bounds.x1 - text.bounds.x0)
                .max(value.chars().count() as f64 * f64::from(run.style.font_size))
        };
        cursor += width;
        if !cursor.is_finite() || cursor > f64::from(right) {
            return None;
        }
    }
    Some(output)
}

/// shaping・kerning・可変fontのない単純cmapだけを、Word half-point用に測定する。
fn simple_advance(bytes: &[u8], text: &str) -> Option<f64> {
    use ttf_parser::{Face, Tag};
    let face = Face::parse(bytes, 0).ok()?;
    for tag in [
        b"GSUB", b"GPOS", b"kern", b"fvar", b"morx", b"mort", b"kerx",
    ] {
        if face.raw_face().table(Tag::from_bytes(tag)).is_some() {
            return None;
        }
    }
    if text.is_empty() || text.chars().count() > 1024 {
        return None;
    }
    let total = text.chars().try_fold(0u64, |total, ch| {
        let glyph = face.glyph_index(ch)?;
        if glyph.0 == 0 {
            return None;
        }
        total.checked_add(u64::from(face.glyph_hor_advance(glyph)?))
    })?;
    (total > 0).then_some(total as f64 / f64::from(face.units_per_em()))
}

/// タブの境界空白だけを置換し、runの書式と非空白文字をそのまま保持する。
pub(super) fn paragraph(runs: &[StructuredTextRun], positions: &[Option<usize>]) -> Paragraph {
    let mut paragraph = Paragraph::new();
    for (run, pos) in runs.iter().zip(positions) {
        if let Some(pos) = pos {
            paragraph = paragraph.add_tab(
                docx_rs::Tab::new()
                    .val(docx_rs::TabValueType::Left)
                    .pos(*pos),
            );
            paragraph = paragraph.add_run(Run::new().add_tab());
            let mut trimmed = run.clone();
            trimmed.text = trimmed.text.trim_start().to_string();
            paragraph = paragraph.add_run(native_table_text::cell_run(&trimmed));
        } else {
            paragraph = paragraph.add_run(native_table_text::cell_run(run));
        }
    }
    paragraph
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 名称とface属性だけを持ち、実際の字形幅が確認できないセル行を作る。
    fn unmeasured() -> (Vec<ParsedTextElement>, StructuredTextLine) {
        let texts = [("A", 10.), ("B", 100.)]
            .map(|(text, x)| ParsedTextElement {
                source_background: None,
                fill_stroke_width: None,
                font_weight: None,
                baseline_y: Some(50.),
                page: 0,
                bounds: ParsedBounds {
                    x0: x,
                    x1: x + 10.,
                    y0: 50.,
                    y1: 60.,
                },
                font_size: 10.,
                font_name: Some("Unmeasured".into()),
                fill_color: None,
                text: text.into(),
            })
            .to_vec();
        let row = texts.iter().collect::<Vec<_>>();
        let mut runs = structured_text_runs_from_row(&row, &[]);
        for run in &mut runs {
            run.style.font_embedded = true;
            run.style.embedded_face = Some(embedded_face::Face::default());
        }
        let line = StructuredTextLine {
            top_y: 60.,
            left_x: 10.,
            right_x: 110.,
            alignment: ParagraphAlignment::Left,
            text: runs.iter().map(|run| run.text.as_str()).collect(),
            runs,
            split_from_row: false,
        };
        (texts, line)
    }

    /// 埋め込みslot名だけを信用せず、実際のadvanceを読めないfontでは旧配置を残す。
    #[test]
    fn unmeasured_embedded_font_does_not_claim_tab_clearance() {
        let (texts, line) = unmeasured();
        assert!(positions(&texts.iter().collect::<Vec<_>>(), &line, 0., 200., &[]).is_none());
    }

    /// 所有fontの一つのcmapをAへ割り当て、一文字一emの実測可能なfontを作る。
    fn owned_font() -> ParsedEmbeddedFont {
        let mut bytes = layout_space::font("Unmeasured");
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        let at = face
            .raw_face()
            .table_records
            .into_iter()
            .find(|record| record.tag == ttf_parser::Tag::from_bytes(b"cmap"))
            .unwrap()
            .offset as usize;
        for (offset, value) in [(26, 65u16), (32, 65), (36, (-64i16) as u16)] {
            bytes[at + offset..at + offset + 2].copy_from_slice(&value.to_be_bytes());
        }
        ParsedEmbeddedFont {
            font_name: "Unmeasured".into(),
            bytes,
        }
    }

    /// 実測advanceが次のstopやセル右端へ達する場合は、原本boxが小さくても適用しない。
    #[test]
    fn actual_glyph_width_must_fit_before_every_stop_and_cell_end() {
        let font = owned_font();
        let (mut texts, mut line) = unmeasured();
        texts[1].text = "A".into();
        line.runs[1].text = "A".into();
        let check = |texts: &[ParsedTextElement], line: &StructuredTextLine| {
            positions(
                &texts.iter().collect::<Vec<_>>(),
                line,
                0.,
                200.,
                std::slice::from_ref(&font),
            )
        };
        assert_eq!(check(&texts, &line), Some(vec![None, Some(2000)]));
        for index in [0, 1] {
            let mut texts = texts.clone();
            let mut line = line.clone();
            texts[index].text = "A".repeat(16);
            line.runs[index].text = texts[index].text.clone();
            assert!(check(&texts, &line).is_none());
        }
        for (index, value) in [(0, 0.), (0, f32::NAN), (1, f32::INFINITY)] {
            let mut texts = texts.clone();
            texts[index].bounds.x1 = value;
            assert!(check(&texts, &line).is_none());
        }
        line.runs[1].text = "B".into();
        texts[1].text = "B".into();
        assert!(check(&texts, &line).is_none());
    }
}
