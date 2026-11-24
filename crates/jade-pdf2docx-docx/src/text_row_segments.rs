//! 添字復元をまたいで、本文の水平分割の根拠を保持する。
use super::{ParsedTextElement, TextRowSegment, text_column_gutter};

/// 添字を復元する前の本文行から、元の水平分割位置を確定する。
pub(super) fn cuts(
    row: &[&ParsedTextElement],
    gutter: Option<text_column_gutter::Gutter>,
) -> Vec<f32> {
    let Some(first) = row.first() else {
        return Vec::new();
    };
    let avg_height = row
        .iter()
        .map(|text| (text.bounds.y1 - text.bounds.y0).abs().max(1.0))
        .sum::<f32>()
        / row.len() as f32;
    let mut right = first.bounds.x1;
    let mut cuts = Vec::new();
    for text in row.iter().skip(1) {
        if text.bounds.x0 - right > avg_height * 8.0
            || gutter.is_some_and(|g| right < g.x && g.x < text.bounds.x0)
        {
            cuts.push(right / 2.0 + text.bounds.x0 / 2.0);
        }
        right = right.max(text.bounds.x1);
    }
    cuts
}

/// 復元した添字で尺度を再推定せず、元の行の分割位置を使う。
pub(super) fn split<'a>(
    row: &[&'a ParsedTextElement],
    gutter: Option<text_column_gutter::Gutter>,
    cuts: &[f32],
) -> Vec<TextRowSegment<'a>> {
    let Some(first) = row.first() else {
        return Vec::new();
    };
    let mut segments: Vec<Vec<&ParsedTextElement>> = vec![vec![*first]];
    let mut bin = cuts.partition_point(|cut| *cut <= first.bounds.x0);
    for text in row.iter().copied().skip(1) {
        let next = cuts.partition_point(|cut| *cut <= text.bounds.x0);
        if next != bin {
            segments.push(vec![text]);
        } else if let Some(segment) = segments.last_mut() {
            segment.push(text);
        }
        bin = next;
    }
    let split_from_row = segments.len() > 1 || gutter.is_some_and(|gutter| gutter.contains(first));
    segments
        .into_iter()
        .map(|texts| TextRowSegment {
            texts,
            split_from_row,
        })
        .collect()
}
