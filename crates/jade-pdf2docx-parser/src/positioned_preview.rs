//! 確認済みの正立glyphだけを有界な行へ戻し、previewの文字単位の重複除去を防ぐ。
use crate::{ParsedTextElement, decode::DecodeBudget};

/// 従来の断片は借用し、位置付きType1行だけを最大1024文字の作業領域で連結する。
pub(super) fn visit(
    budget: &DecodeBudget,
    elements: &[ParsedTextElement],
    mut candidate: impl FnMut(&str) -> Option<bool>,
) -> Option<()> {
    let mut row = String::new();
    let mut chars = 0usize;
    let mut previous: Option<&ParsedTextElement> = None;
    for element in elements {
        if budget.is_stopped() {
            return None;
        }
        let upright = element.baseline_y.is_some_and(f32::is_finite);
        let derived = element
            .font_name
            .as_deref()
            .is_some_and(|name| name.starts_with("JadeT1-"));
        let logical_space =
            previous.is_some() && element.font_name.is_none() && element.text == " ";
        let group = upright && (derived || logical_space);
        let adjacent = previous.is_none_or(|prior| {
            prior.page == element.page
                && prior.baseline_y == element.baseline_y
                && element.bounds.x0 >= prior.bounds.x0
                && element.bounds.x0 - prior.bounds.x1
                    <= prior.font_size.max(element.font_size) * 0.8
        });
        if !group || !adjacent {
            if !row.is_empty() && !candidate(&row)? {
                return Some(());
            }
            row.clear();
            chars = 0;
            previous = None;
        }
        if !group {
            if !candidate(&element.text)? {
                return Some(());
            }
            continue;
        }
        let text = if row.is_empty() {
            element.text.trim_start()
        } else {
            &element.text
        };
        let end = text
            .char_indices()
            .nth(1024 - chars)
            .map_or(text.len(), |(offset, _)| offset);
        let part = &text[..end];
        crate::text_result_storage::extend(budget, &mut row, part)?;
        chars += part.chars().count();
        previous = Some(element);
        if chars == 1024 {
            if !candidate(&row)? {
                return Some(());
            }
            row.clear();
            chars = 0;
            previous = None;
        }
    }
    if !row.is_empty() {
        candidate(&row)?;
    }
    Some(())
}
