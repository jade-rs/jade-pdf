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

/// 同じobject幾何・style・baselineの仮spanを作る。採用には元矩形との完全照合が別途必要。
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
        && previous.font_weight == next.font_weight
        && previous.font_name == next.font_name
        && previous.fill_color == next.fill_color
        && previous.baseline_y.is_some_and(|v| v.is_finite())
        && previous.baseline_y == next.baseline_y
        && [a.x0, a.y0, a.x1, a.y1, b.x0, b.y0, b.x1, b.y1]
            .iter()
            .all(|v| v.is_finite());
    if !coherent {
        return Ok(false);
    }
    crate::text_result_storage::extend(budget, &mut previous.text, &next.text);
    budget.check()?;
    previous.bounds.x0 = a.x0.min(b.x0);
    previous.bounds.x1 = a.x1.max(b.x1);
    previous.bounds.y0 = a.y0.min(b.y0);
    previous.bounds.y1 = a.y1.max(b.y1);
    Ok(true)
}

/// 本文・style・baselineとobject外接矩形の一致を判定する借用キー。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Signature<'a> {
    /// 端のSDK空白だけを除く、元の本文。
    text: &'a str,
    /// font・色・ページを含む、先頭文字のstyle。
    style: (usize, u32, Option<&'a str>, Option<&'a str>, Option<u32>),
    /// 明示されたdescriptor値を、未確認や別の太さと区別する。
    weight: Option<u16>,
    /// 文字loose boundsではなく、元矩形に対応するobject幾何。
    bounds: [u32; 4],
}

/// styleと矩形を含め、別位置の同じ字面を承認に使わない。
fn signature(text: &ParsedTextElement, bounds: [f32; 4]) -> Signature<'_> {
    Signature {
        text: text.text.trim(),
        style: (
            text.page,
            text.font_size.to_bits(),
            text.font_name.as_deref(),
            text.fill_color.as_deref(),
            text.baseline_y.map(f32::to_bits),
        ),
        weight: text.font_weight,
        bounds: bounds.map(f32::to_bits),
    }
}

/// 既存文字要素を予算付きで仮spanへ複製し、失敗を公開エラーへ伝える。
fn duplicate(
    budget: &DecodeBudget,
    text: &ParsedTextElement,
) -> Result<ParsedTextElement, ParseError> {
    let content = crate::text_result_storage::copy(budget, Some(&text.text));
    let font = crate::text_result_storage::copy(budget, text.font_name.as_deref());
    let color = crate::text_result_storage::copy(budget, text.fill_color.as_deref());
    budget.check()?;
    Ok(ParsedTextElement {
        text: content.flatten().unwrap_or_default(),
        font_name: font.flatten(),
        fill_color: color.flatten(),
        ..*text
    })
}

/// 元矩形と全文・style・幾何が一致するobject spanだけを採用し、その他は各glyphを保持する。
/// source本文集合と各文字列は共有予算へ算入し、SDK全文・全objectを反復走査しない。
pub(super) fn retain(
    budget: &DecodeBudget,
    elements: Vec<ParsedTextElement>,
    keys: &[(Option<Key>, bool)],
    original: &[ParsedTextElement],
) -> Result<Vec<ParsedTextElement>, ParseError> {
    let mut source = crate::metadata_workspace::Set::default();
    for text in original {
        budget.check()?;
        let b = text.bounds;
        source.insert(budget, signature(text, [b.x0, b.y0, b.x1, b.y1]))?;
    }
    let mut accepted = Vec::new();
    let mut start = 0;
    while start < elements.len() {
        budget.check()?;
        let mut span = duplicate(budget, &elements[start])?;
        let key = keys[start].0;
        let mut end = start + 1;
        while end < elements.len()
            && keys[end].1
            && join(budget, &mut span, &elements[end], key, keys[end].0)?
        {
            end += 1;
        }
        if end > start + 1
            && let Some(key) = key
        {
            let bounds = [
                key.geometry[0],
                key.geometry[1],
                key.geometry[2],
                key.geometry[3],
            ];
            if source.contains(&signature(&span, bounds)) {
                span.bounds = crate::ParsedBounds {
                    x0: bounds[0],
                    y0: bounds[1],
                    x1: bounds[2],
                    y1: bounds[3],
                };
                crate::metadata_workspace::push(budget, &mut accepted, (start, end, span))?;
            }
        }
        start = end;
    }
    let mut output = crate::metadata_workspace::vector(budget, elements.len())?;
    let mut accepted = accepted.into_iter().peekable();
    let mut skip_until = 0;
    for (index, element) in elements.into_iter().enumerate() {
        budget.check()?;
        if accepted.peek().is_some_and(|(start, _, _)| *start == index) {
            let (_, end, span) = accepted.next().unwrap();
            output.push(span);
            skip_until = end;
        } else if index >= skip_until {
            output.push(element);
        }
    }
    Ok(output)
}
