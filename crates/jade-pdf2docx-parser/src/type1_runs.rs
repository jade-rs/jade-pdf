//! 元codeごとの位置を保持し、Unicode文字数で元glyphのadvanceを変えない。
use super::type1_fonts::Routes;
use crate::{decode::DecodeBudget, font_widths::Metrics, text_draws::State};

/// 検証済み元glyphを一つずつ描画し、Tc/Tw/TzとTJによる位置を出力へ残す。
pub(super) fn show(
    budget: &DecodeBudget,
    raw: &[u8],
    routes: &Routes,
    state: &State,
    matrix: [f32; 6],
    metrics: Option<&Metrics>,
    mut draw: impl FnMut(&[u8], [f32; 6], Option<&str>) -> f32,
) -> f32 {
    // 壊れた末尾を見つける前に、部分的なglyphを公開しない。
    if crate::pdf_binary_string::visit_string(raw, |_| (!budget.is_stopped()).then_some(()))
        .is_none()
    {
        return 0.;
    }
    let Some(metrics) = metrics else {
        return draw(raw, matrix, None);
    };
    let mut advance = 0_f32;
    let valid = crate::pdf_binary_string::visit_string(raw, |code| {
        if budget.is_stopped() {
            return None;
        }
        let byte = [code];
        let mut remaining = byte.as_slice();
        let (_, width, space) = metrics.next_glyph(&mut remaining, state.size)?;
        let operand = [
            b'<',
            b"0123456789ABCDEF"[usize::from(code >> 4)],
            b"0123456789ABCDEF"[usize::from(code & 15)],
            b'>',
        ];
        draw(
            &operand,
            translated(matrix, advance * state.horizontal_scale),
            routes.family(routes.codes[usize::from(code)]),
        );
        advance =
            advance + width + state.character_spacing + if space { state.word_spacing } else { 0. };
        advance.is_finite().then_some(())
    });
    if valid.is_none() || budget.is_stopped() {
        return 0.;
    }
    advance * state.horizontal_scale
}

/// Tz適用済みの原点移動だけを行い、glyphの拡大率を二重に適用しない。
fn translated(matrix: [f32; 6], advance: f32) -> [f32; 6] {
    super::concat_matrix(matrix, [1., 0., 0., 1., advance, 0.])
}
