//! 同じ行の抽出単位に依存せず、水平罫線と文字下線の既存対応を保持する。
use super::{ParsedGraphicElement, ParsedTextElement, is_horizontal_line, normalized_bounds};

/// 有限の行全体の横範囲を一度だけ計算し、無効値では既存の断片単位の上限へ戻す。
pub(super) fn row_width(row: &[&ParsedTextElement]) -> f32 {
    row.iter()
        .try_fold((f32::INFINITY, f32::NEG_INFINITY), |(left, right), text| {
            let bounds = normalized_bounds(text.bounds);
            (bounds.x0.is_finite() && bounds.x1.is_finite())
                .then_some((left.min(bounds.x0), right.max(bounds.x1)))
        })
        .map(|(left, right)| right - left)
        .filter(|width| width.is_finite() && *width > 0.)
        .unwrap_or(0.)
}

/// PDF text の直下にある短い水平線を文字下線として扱う。
pub(super) fn parsed_text_has_underline(
    text: &ParsedTextElement,
    graphics: &[ParsedGraphicElement],
) -> bool {
    parsed_text_has_underline_with_row_width(text, graphics, 0.)
}

/// 行内の分割前の幅を上限判定だけへ使い、各断片の直下にある線だけを下線とする。
pub(super) fn parsed_text_has_underline_with_row_width(
    text: &ParsedTextElement,
    graphics: &[ParsedGraphicElement],
    row_width: f32,
) -> bool {
    let text_bounds = normalized_bounds(text.bounds);
    let text_width = (text_bounds.x1 - text_bounds.x0).abs().max(1.0);
    let context_width = if row_width.is_finite() && row_width > text_width {
        row_width
    } else {
        text_width
    };
    let text_height = (text_bounds.y1 - text_bounds.y0)
        .abs()
        .max(text.font_size.max(1.0));
    let x_tolerance = text_height * 0.6;
    let min_y = text_bounds.y0 - text_height * 0.7;
    let max_y = text_bounds.y0 + text_height * 0.35;
    graphics.iter().any(|graphic| {
        if graphic.page != text.page {
            return false;
        }
        let bounds = normalized_bounds(graphic.bounds);
        if !is_horizontal_line(&bounds) {
            return false;
        }
        let line_width = bounds.x1 - bounds.x0;
        if line_width < text_width * 0.35 || line_width > (context_width * 8.0).max(180.0) {
            return false;
        }
        let line_y = bounds.y0;
        let text_center_x = (text_bounds.x0 + text_bounds.x1) / 2.0;
        line_y >= min_y
            && line_y <= max_y
            && text_center_x >= bounds.x0 - x_tolerance
            && text_center_x <= bounds.x1 + x_tolerance
            && bounds.x0 <= text_bounds.x1 + x_tolerance
            && bounds.x1 >= text_bounds.x0 - x_tolerance
    })
}
