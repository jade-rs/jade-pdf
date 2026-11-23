//! 元codeの連続runだけを分け、原本glyph数によるadvanceと文字状態を保つ。
use super::type1_fonts::Routes;
use crate::{decode::DecodeBudget, font_widths::Metrics, text_draws::State};

/// 必要なときだけhex operandを一つ予約し、混在runを元の行列位置へ描画する。
pub(super) fn show(
    budget: &DecodeBudget,
    raw: &[u8],
    routes: &Routes,
    state: &State,
    matrix: [f32; 6],
    metrics: Option<&Metrics>,
    mut draw: impl FnMut(&[u8], [f32; 6], Option<&str>) -> f32,
) -> f32 {
    let mut first = None;
    let mut previous = None;
    let mut mixed = false;
    let mut current = 0usize;
    let mut longest = 0usize;
    let valid = crate::pdf_binary_string::visit_string(raw, |code| {
        if budget.is_stopped() {
            return None;
        }
        let face = routes.codes[usize::from(code)];
        if first.is_none() {
            first = Some(face);
        }
        if previous == Some(face) {
            current += 1;
        } else {
            longest = longest.max(current);
            current = 1;
            previous = Some(face);
        }
        mixed |= Some(face) != first;
        Some(())
    });
    if valid.is_none() {
        return 0.;
    }
    if !mixed {
        return draw(raw, matrix, routes.family(first.flatten()));
    }
    let Some(metrics) = metrics else {
        return draw(raw, matrix, None);
    };
    longest = longest.max(current);
    let Some(capacity) = longest.checked_mul(2).and_then(|n| n.checked_add(2)) else {
        return 0.;
    };
    let Some(mut buffer) = budget.metadata_vector::<u8>(capacity) else {
        return 0.;
    };
    buffer.push(b'<');
    let mut face = first.flatten();
    let mut advance = 0_f32;
    let mut start = 0_f32;
    let valid = crate::pdf_binary_string::visit_string(raw, |code| {
        if budget.is_stopped() {
            return None;
        }
        let next = routes.codes[usize::from(code)];
        if next != face {
            buffer.push(b'>');
            draw(
                &buffer,
                translated(matrix, start * state.horizontal_scale),
                routes.family(face),
            );
            buffer.clear();
            buffer.push(b'<');
            face = next;
            start = advance;
        }
        buffer.push(b"0123456789ABCDEF"[usize::from(code >> 4)]);
        buffer.push(b"0123456789ABCDEF"[usize::from(code & 15)]);
        let byte = [code];
        let mut remaining = byte.as_slice();
        let (_, width, space) = metrics.next_glyph(&mut remaining, state.size)?;
        advance =
            advance + width + state.character_spacing + if space { state.word_spacing } else { 0. };
        advance.is_finite().then_some(())
    });
    if valid.is_none() || budget.is_stopped() {
        return 0.;
    }
    buffer.push(b'>');
    draw(
        &buffer,
        translated(matrix, start * state.horizontal_scale),
        routes.family(face),
    );
    advance * state.horizontal_scale
}

/// Tz適用済みの原点移動だけを行い、glyphの拡大率を二重に適用しない。
fn translated(matrix: [f32; 6], advance: f32) -> [f32; 6] {
    super::concat_matrix(matrix, [1., 0., 0., 1., advance, 0.])
}
