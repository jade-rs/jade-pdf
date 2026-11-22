//! 罫線本文を借用byteで走査し、作業・結果配列の全容量を予約する。
use super::{
    DecodeBudget, ParsedBounds, ParsedGraphicElement, concat_matrix, is_axis_aligned,
    normalized_bounds, thin_path_segments_to_line, thin_rect_to_line, transform_point,
    transform_rect_bounds,
};
use crate::syntax::{Lexer, Token};

/// q/Qで保存・復元する罫線描画状態。
#[derive(Clone, Copy)]
struct State {
    /// 現在の座標変換行列。
    matrix: [f32; 6],
    /// 現在の線幅。
    width: f32,
}

impl Default for State {
    /// 恒等行列とPDFの既定線幅を返す。
    fn default() -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            width: 1.0,
        }
    }
}

/// 初回4slot・倍増後の全容量を予約して値を移す。
fn push<T>(budget: &DecodeBudget, values: &mut Vec<T>, value: T) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    if values.len() == values.capacity() {
        let capacity = values.capacity().saturating_mul(2).max(4);
        let mut replacement = budget.metadata_vector(capacity)?;
        replacement.append(values);
        *values = replacement;
    }
    values.push(value);
    Some(())
}

/// 有限座標だけを結果へ保存し、全slotを確保前に予約する。
fn emit(
    budget: &DecodeBudget,
    output: &mut Vec<ParsedGraphicElement>,
    page: usize,
    bounds: ParsedBounds,
    width: f32,
) -> Option<()> {
    if [bounds.x0, bounds.y0, bounds.x1, bounds.y1, width]
        .iter()
        .all(|v| v.is_finite())
    {
        push(
            budget,
            output,
            ParsedGraphicElement {
                page,
                bounds,
                stroke_width: width,
            },
        )?;
    }
    Some(())
}

/// 現在の点からサブパスの始点へ戻る辺を加え、現在の点を始点へ移す。
fn close_subpath(
    budget: &DecodeBudget,
    segments: &mut Vec<ParsedBounds>,
    point: &mut Option<(f32, f32)>,
    start: Option<(f32, f32)>,
) -> Option<()> {
    if let (Some((x0, y0)), Some((x1, y1))) = (*point, start)
        && (x0 != x1 || y0 != y1)
    {
        push(budget, segments, ParsedBounds { x0, y0, x1, y1 })?;
    }
    *point = start;
    Some(())
}

/// 矩形の 4 辺のうち、長さのある辺を返す。
fn rectangle_edges(x: f32, y: f32, w: f32, h: f32) -> Vec<ParsedBounds> {
    let (x0, x1) = (x.min(x + w), x.max(x + w));
    let (y0, y1) = (y.min(y + h), y.max(y + h));
    let mut edges = Vec::with_capacity(4);
    if x1 > x0 {
        edges.push(ParsedBounds { x0, y0, x1, y1: y0 });
        edges.push(ParsedBounds { x0, y0: y1, x1, y1 });
    }
    if y1 > y0 {
        edges.push(ParsedBounds { x0, y0, x1: x0, y1 });
        edges.push(ParsedBounds { x0: x1, y0, x1, y1 });
    }
    edges
}

/// ページをまたぐ結果配列へ追記する。失敗時は呼び出し元が全結果を破棄する。
pub(super) fn append(
    budget: &DecodeBudget,
    page: usize,
    content: &[u8],
    output: &mut Vec<ParsedGraphicElement>,
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut operands = [0.0f32; 6];
    let mut count = 0usize;
    let mut depth = 0usize;
    let mut state = State::default();
    let mut saved = Vec::new();
    let mut point = None;
    let mut start = None;
    let mut segments = Vec::new();
    let mut rectangles = Vec::new();
    while let Some(token) = lexer.next() {
        match token {
            Token::Open(_) => {
                depth = depth.saturating_add(1);
                count = 0;
                continue;
            }
            Token::Close(_) => {
                depth = depth.saturating_sub(1);
                count = 0;
                continue;
            }
            _ if depth > 0 => continue,
            _ => {}
        }
        let Token::Word(word) = token else {
            count = 0;
            continue;
        };
        if let Some(number) = crate::page_attributes::number_value(word) {
            if count == 6 {
                operands.copy_within(1..6, 0);
                count = 5;
            }
            operands[count] = number;
            count += 1;
            continue;
        }
        let args = &operands[..count];
        match word {
            b"m" | b"l" if count >= 2 => {
                let (x, y) = transform_point(state.matrix, args[count - 2], args[count - 1]);
                if x.is_finite() && y.is_finite() {
                    if word == b"l"
                        && let Some((x0, y0)) = point
                    {
                        push(
                            budget,
                            &mut segments,
                            ParsedBounds {
                                x0,
                                y0,
                                x1: x,
                                y1: y,
                            },
                        )?;
                    }
                    point = Some((x, y));
                    if word == b"m" {
                        start = point;
                    }
                } else {
                    point = None;
                }
            }
            b"h" => close_subpath(budget, &mut segments, &mut point, start)?,
            b"re" if count >= 4 => {
                let a = &args[count - 4..];
                let b = transform_rect_bounds(state.matrix, a[0], a[1], a[2], a[3]);
                let rect = [b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0];
                if rect.iter().all(|v| v.is_finite()) {
                    push(budget, &mut rectangles, rect)?;
                }
            }
            b"cm" if count == 6 => {
                state.matrix = concat_matrix(state.matrix, operands);
            }
            b"q" => push(budget, &mut saved, state)?,
            b"Q" => {
                if let Some(previous) = saved.pop() {
                    state = previous;
                }
            }
            b"w" if count > 0 => {
                state.width = args[count - 1];
            }
            // 線を描く演算子（塗りと同時のものを含む）。`s`・`b`・`b*` はパスを閉じてから描く。
            b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*" => {
                if matches!(word, b"s" | b"b" | b"b*") {
                    close_subpath(budget, &mut segments, &mut point, start)?;
                }
                for segment in segments.drain(..) {
                    if is_axis_aligned(segment) {
                        emit(
                            budget,
                            output,
                            page,
                            normalized_bounds(segment),
                            state.width,
                        )?;
                    }
                }
                // 矩形の枠線（セル枠を矩形で描く表など）は 4 辺の罫線として出す。
                for [x, y, w, h] in rectangles.drain(..) {
                    for edge in rectangle_edges(x, y, w, h) {
                        emit(budget, output, page, edge, state.width)?;
                    }
                }
                point = None;
                start = None;
            }
            b"f" | b"f*" | b"F" => {
                for [x, y, w, h] in rectangles.drain(..) {
                    if let Some((bounds, width)) = thin_rect_to_line(x, y, w, h) {
                        emit(budget, output, page, bounds, width)?;
                    }
                }
                if let Some((bounds, width)) = thin_path_segments_to_line(&segments) {
                    emit(budget, output, page, bounds, width)?;
                }
                segments.clear();
                point = None;
            }
            b"n" => {
                segments.clear();
                rectangles.clear();
                point = None;
                start = None;
            }
            // 未対応のinline画像のbinary本文を命令として解釈しない。
            b"BI" => break,
            _ => {}
        }
        count = 0;
    }
    Some(())
}

#[cfg(test)]
#[path = "graphics_draws_tests.rs"]
mod tests;
