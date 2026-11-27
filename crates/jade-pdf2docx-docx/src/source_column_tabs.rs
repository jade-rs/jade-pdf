//! 独立した段の項目名と数値を、原本の水平位置を保つ編集可能なタブで組む。
use super::*;

/// 標準fontの確実な二断片だけで、数値を原本の横位置に置く。
pub(super) fn paragraph(line: &StructuredTextLine, origin: f64, end: f64) -> Option<Paragraph> {
    let [label, value] = line.runs.as_slice() else {
        return None;
    };
    let label_bounds = label.style.source_bounds?;
    let value_bounds = value.style.source_bounds?;
    if !text_row_segments::numeric_fragment(&value.text)
        || value_bounds.x0 - label_bounds.x1
            <= label.style.font_size.max(value.style.font_size) * 2.
    {
        return None;
    }
    let label_end = section_text_end(label, label_bounds)?;
    let value_end = section_text_end(value, value_bounds)?;
    let target = f64::from(value_bounds.x0) * 20.;
    let position = (target - origin).round();
    if position <= 0. || label_end + 1. >= target || value_end >= end {
        return None;
    }
    let mut value = value.clone();
    value.text = value.text.trim_start().to_owned();
    Some(
        Paragraph::new()
            .add_tab(
                docx_rs::Tab::new()
                    .val(docx_rs::TabValueType::Left)
                    .pos(position as usize),
            )
            .add_run(docx_run_from_structured_run(label))
            .add_run(Run::new().add_tab())
            .add_run(docx_run_from_structured_run(&value)),
    )
}

/// Word上でもstopを越えないよう、対応する標準fontを一字一emで保守的に見積もる。
fn section_text_end(run: &StructuredTextRun, bounds: ParsedBounds) -> Option<f64> {
    let style = &run.style;
    if !matches!(style.font_name.as_deref(), Some("Courier" | "Helvetica"))
        || style.font_embedded
        || style.fill_stroke_width.is_some()
        || style.font_weight.is_some_and(|weight| weight > 500)
        || ![bounds.x0, bounds.x1, style.font_size]
            .iter()
            .all(|v| v.is_finite())
        || bounds.x1 <= bounds.x0
        || style.font_size <= 0.
        || (style.font_size * 2.).fract() != 0.
        || !run.text.bytes().all(|c| (b' '..=b'~').contains(&c))
    {
        return None;
    }
    let estimated_end =
        f64::from(bounds.x0) + run.text.trim_start().len() as f64 * f64::from(style.font_size);
    Some(estimated_end.max(f64::from(bounds.x1)) * 20.)
}
