//! 閉じたセルの一行にある明確な空隙を、文字群の原点を持つWordタブへ渡す。
use super::*;

/// 本文・font・幾何を全断片で確認し、大きな空隙がある行だけ局所位置を返す。
pub(super) fn positions(
    row: &[&ParsedTextElement],
    line: &StructuredTextLine,
    left: f32,
    right: f32,
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
    large_gap.then_some(output)
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
