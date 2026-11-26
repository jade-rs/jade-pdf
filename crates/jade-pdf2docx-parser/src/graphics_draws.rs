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
    /// Device色で確認した非strokeの白。未知の色命令で解除する。
    white_fill: bool,
}

/// q保存の全slotを検査するための実際の状態容量。
#[cfg(test)]
pub(super) const SAVED_STATE_BYTES: usize = size_of::<State>();

impl Default for State {
    /// 恒等行列とPDFの既定線幅を返す。
    fn default() -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            width: 1.0,
            white_fill: false,
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

/// ページをまたぐ結果配列へ追記する。失敗時は呼び出し元が全結果を破棄する。
#[cfg(test)]
pub(super) fn append(
    budget: &DecodeBudget,
    page: usize,
    content: &[u8],
    output: &mut Vec<ParsedGraphicElement>,
) -> Option<()> {
    append_filtered(budget, page, content, output, |_| false)
}

/// 白の描画候補だけで所属ページの合成・色変換を遅延検証する。
pub(super) fn append_filtered(
    budget: &DecodeBudget,
    page: usize,
    content: &[u8],
    output: &mut Vec<ParsedGraphicElement>,
    mut confirmed_white: impl FnMut(usize) -> bool,
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
    let mut segments = Vec::new();
    let mut rectangles = Vec::new();
    // 曲線を直線近似しない。途中の線分だけから罫線を推測することも避ける。
    let mut curved = false;
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
            b"g" => state.white_fill = args.len() == 1 && args[0] >= 1.0,
            b"rg" => state.white_fill = args.len() == 3 && args.iter().all(|v| *v >= 1.0),
            b"k" => state.white_fill = args.len() == 4 && args.iter().all(|v| *v <= 0.0),
            b"cs" | b"sc" | b"scn" => state.white_fill = false,
            _ => {}
        }
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
                } else {
                    point = None;
                }
            }
            b"re" if count >= 4 => {
                let a = &args[count - 4..];
                let b = transform_rect_bounds(state.matrix, a[0], a[1], a[2], a[3]);
                let rect = [b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0];
                if rect.iter().all(|v| v.is_finite()) {
                    push(budget, &mut rectangles, rect)?;
                }
            }
            b"c" | b"v" | b"y" => {
                curved = true;
                point = None;
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
            b"S" => {
                for segment in segments.drain(..) {
                    if !curved && is_axis_aligned(segment) {
                        emit(
                            budget,
                            output,
                            page,
                            normalized_bounds(segment),
                            state.width,
                        )?;
                    }
                }
                rectangles.clear();
                point = None;
                curved = false;
            }
            b"f" | b"f*" => {
                for [x, y, w, h] in rectangles.drain(..) {
                    if !curved && let Some((bounds, width)) = thin_rect_to_line(x, y, w, h) {
                        if !(state.white_fill && confirmed_white(lexer.cursor)) {
                            emit(budget, output, page, bounds, width)?;
                        }
                        if budget.is_stopped() {
                            return None;
                        }
                    }
                }
                if !curved && let Some((bounds, width)) = thin_path_segments_to_line(&segments) {
                    if !(state.white_fill && confirmed_white(lexer.cursor)) {
                        emit(budget, output, page, bounds, width)?;
                    }
                    if budget.is_stopped() {
                        return None;
                    }
                }
                segments.clear();
                point = None;
                curved = false;
            }
            b"n" | b"s" | b"F" | b"B" | b"B*" | b"b" | b"b*" => {
                segments.clear();
                rectangles.clear();
                point = None;
                curved = false;
            }
            // 未対応のinline画像のbinary本文を命令として解釈しない。
            b"BI" => break,
            _ => {}
        }
        count = 0;
    }
    Some(())
}
