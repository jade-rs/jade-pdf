//! encoded glyphの幅・Unicode mappingを保持して、clip外の本文だけを除外する。
use crate::{
    ParsedBounds, decode::DecodeBudget, font_widths::Metrics, text::ToUnicodeCMap,
    text_geometry::TextGeometry,
};

/// 正確なglyph幅が分かるoperandを切り抜く。未対応幅は従来の全体矩形へ委ねる。
#[allow(clippy::too_many_arguments)]
pub(crate) fn filter(
    budget: &DecodeBudget,
    raw: &[u8],
    metrics: &Metrics,
    geometry: TextGeometry<'_>,
    size: f32,
    character_spacing: f32,
    word_spacing: f32,
    cmap: &ToUnicodeCMap,
    encoding: Option<&str>,
) -> Option<(String, Option<ParsedBounds>)> {
    let bytes = crate::text::pdf_string_bytes(budget, raw)?;
    // 未対応codeが末尾にあっても、一部だけの復号結果を採用しない。
    let mut remaining = bytes.as_slice();
    while !remaining.is_empty() {
        if budget.is_stopped() {
            return None;
        }
        let (_, width, _) = metrics.next_glyph(&mut remaining, size)?;
        if !width.is_finite() {
            return None;
        }
    }
    let mut remaining = bytes.as_slice();
    let mut offset = 0.;
    let mut text = String::new();
    let mut bounds: Option<ParsedBounds> = None;
    while !remaining.is_empty() {
        if budget.is_stopped() {
            return None;
        }
        let (code, width, space) = metrics.next_glyph(&mut remaining, size)?;
        if let Some(glyph) = geometry
            .transformed([1., 0., 0., 1., offset, 0.])
            .bounds(budget, width, size)
        {
            let value = crate::text::decode_glyph_bytes(
                budget,
                code,
                raw.first() == Some(&b'('),
                cmap,
                encoding,
            )?;
            crate::text_result_storage::extend(budget, &mut text, &value)?;
            bounds = Some(match bounds {
                None => glyph,
                Some(previous) => ParsedBounds {
                    x0: previous.x0.min(glyph.x0),
                    y0: previous.y0.min(glyph.y0),
                    x1: previous.x1.max(glyph.x1),
                    y1: previous.y1.max(glyph.y1),
                },
            });
        }
        offset += width + character_spacing + if space { word_spacing } else { 0. };
        if !offset.is_finite() {
            return None;
        }
    }
    Some((text, bounds))
}
