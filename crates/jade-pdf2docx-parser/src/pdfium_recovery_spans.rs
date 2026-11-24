//! SDKの公開object幾何で連続文字を照合し、文字indexを再走査せずspanを保持する。
use crate::{ParseError, ParsedTextElement, decode::DecodeBudget};
use pdfium_render::prelude::*;

#[cfg(test)]
#[path = "pdfium_recovery_spans_tests.rs"]
mod tests;

/// objectの有限な外接矩形と変換行列を照合する固定長の幾何fingerprint。
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Key {
    /// SDK公開APIから得た外接矩形と行列。private handleの同一性は仮定しない。
    geometry: [f32; 10],
}

/// 水平で正方向のobjectだけを対象に、各文字あたり定数回のSDK呼び出しで取得する。
pub(super) fn key(character: &PdfPageTextChar<'_>) -> Option<Key> {
    let object = character.text_object().ok()?;
    let bounds = object.bounds().ok()?.to_rect();
    let matrix = object.matrix().ok()?;
    let geometry = [
        bounds.left().value,
        bounds.bottom().value,
        bounds.right().value,
        bounds.top().value,
        matrix.a(),
        matrix.b(),
        matrix.c(),
        matrix.d(),
        matrix.e(),
        matrix.f(),
    ];
    (geometry.iter().all(|v| v.is_finite())
        && matrix.a() > 0.
        && matrix.d() > 0.
        && matrix.b() == 0.
        && matrix.c() == 0.)
        .then_some(Key { geometry })
}

/// 同じ幾何・style・baselineで右へ連続する文字だけを、予算付きで結合する。
pub(super) fn join(
    budget: &DecodeBudget,
    previous: &mut ParsedTextElement,
    next: &ParsedTextElement,
    previous_key: Option<Key>,
    next_key: Option<Key>,
) -> Result<bool, ParseError> {
    budget.check()?;
    let size = previous.font_size;
    let a = previous.bounds;
    let b = next.bounds;
    let coherent = previous_key.is_some()
        && previous_key == next_key
        && size.is_finite()
        && size > 0.
        && size == next.font_size
        && previous.page == next.page
        && previous.font_name == next.font_name
        && previous.fill_color == next.fill_color
        && previous.baseline_y.is_some_and(|v| v.is_finite())
        && previous.baseline_y == next.baseline_y
        && [a.x0, a.y0, a.x1, a.y1, b.x0, b.y0, b.x1, b.y1]
            .iter()
            .all(|v| v.is_finite())
        && b.x0 > a.x0
        && b.x1 > a.x1
        && b.x0 >= a.x1 - size * 0.25
        && b.x0 <= a.x1 + size * 0.75;
    if !coherent {
        return Ok(false);
    }
    crate::text_result_storage::extend(budget, &mut previous.text, &next.text);
    budget.check()?;
    previous.bounds.x1 = b.x1;
    previous.bounds.y0 = a.y0.min(b.y0);
    previous.bounds.y1 = a.y1.max(b.y1);
    Ok(true)
}
