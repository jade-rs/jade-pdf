//! native通常抽出の文字indexと結果保存を予算付きで扱う。
use crate::{ParseError, ParsedBounds, ParsedTextElement, decode::DecodeBudget};
use pdfium_render::prelude::*;

#[cfg(test)]
#[path = "pdfium_text_budget_tests.rs"]
mod budget_tests;

/// i32文字indexの全要求容量を予約してから、共有するページ配列を取得する。
pub(crate) fn characters<'a>(
    budget: &DecodeBudget,
    page_text: &'a PdfPageText<'_>,
) -> Result<PdfPageTextChars<'a>, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let count = u64::try_from(page_text.len()).map_err(|_| {
        ParseError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "negative native character count",
        ))
    })?;
    budget.charge_metadata_workspace_bytes(count * std::mem::size_of::<i32>() as u64);
    budget.check()?;
    crate::metadata_workspace::ensure_active(budget)?;
    Ok(page_text.chars())
}

/// segment本文と先頭文字の幾何/styleを、共有indexと予算付き保存でIRへ変換する。
pub(crate) fn extract(
    budget: &DecodeBudget,
    page_number: usize,
    page_text: &PdfPageText<'_>,
    characters: &PdfPageTextChars<'_>,
) -> Result<Vec<ParsedTextElement>, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let mut elements = Vec::new();
    for segment in page_text.segments().iter() {
        crate::metadata_workspace::ensure_active(budget)?;
        // API内部の一時Stringは現APIで確保前の正確な容量取得ができない。
        let raw = segment.text();
        // 描画された空白を単語境界として保持し、SDKの端の改行だけを除く。
        let content = raw.trim_matches(|ch| matches!(ch, '\r' | '\n'));
        if content.is_empty() {
            continue;
        }
        let bounds = parsed_bounds_from_pdf_rect(segment.bounds());
        let mut font_size = bounds.y1 - bounds.y0;
        let mut font_name = None;
        let mut fill_color = None;
        let mut baseline_y = None;
        if let Some(first) = first_in_rect(characters, segment.bounds()) {
            font_size = effective_font_size(&first);
            baseline_y = horizontal_baseline(&first);
            let name = first.font_name();
            font_name = copy(budget, (!name.is_empty()).then_some(name.as_str()))?;
            let color = first.fill_color().ok().map(|color| color.to_hex());
            fill_color = copy(budget, color.as_deref())?;
        }
        let text = copy(budget, Some(content))?.unwrap_or_default();
        crate::metadata_workspace::push(
            budget,
            &mut elements,
            ParsedTextElement {
                baseline_y,
                page: page_number,
                bounds,
                font_size,
                font_name,
                fill_color,
                text,
            },
        )?;
    }
    Ok(elements)
}

/// segment.charsと同じ端点・許容幅で検索し、範囲の先頭だけを無確保で取得する。
/// 両端が見つかる場合は依存先のinclusive範囲（end+1）と同じ空判定を行う。
fn first_in_rect<'a>(
    characters: &'a PdfPageTextChars<'_>,
    rect: PdfRect,
) -> Option<PdfPageTextChar<'a>> {
    let tolerance_x = rect.width() / 2.0;
    let tolerance_y = rect.height() / 2.0;
    let center = rect.bottom() + tolerance_y;
    let start = characters.get_char_near_point(rect.left(), tolerance_x, center, tolerance_y);
    let end = characters.get_char_near_point(rect.right(), tolerance_x, center, tolerance_y);
    match (start, end) {
        (Some(start), Some(end)) if start.index() <= end.index().saturating_add(1) => Some(start),
        (Some(_), Some(_)) => None,
        (Some(start), None) => Some(start),
        (None, Some(end)) => Some(end),
        (None, None) => None,
    }
}

/// 文字行列の縦方向の長さで font size を拡大縮小する。
///
/// Pdfium の `scaled_font_size` は行列の `d` 成分だけを掛けるため、90 度回転した文字では 0 になる。
/// 回転に依らない縦ベクトル `(c, d)` の長さを使い、取得できない場合は未拡大の値を返す。
pub(crate) fn effective_font_size(character: &PdfPageTextChar<'_>) -> f32 {
    let unscaled = character.unscaled_font_size().value;
    let scale = character
        .matrix()
        .ok()
        .map(|matrix| matrix.c().hypot(matrix.d()))
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(1.0);
    unscaled * scale
}

/// 本文・属性の保存コピーを予約し、容量不足を公開エラーへ伝播する。
fn copy(budget: &DecodeBudget, source: Option<&str>) -> Result<Option<String>, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let result = crate::text_result_storage::copy(budget, source);
    budget.check()?;
    result.ok_or_else(|| ParseError::Io(std::io::Error::other("native text storage stopped")))
}

/// Pdfiumの矩形をparser IRのboundsへ変換する。
fn parsed_bounds_from_pdf_rect(rect: PdfRect) -> ParsedBounds {
    ParsedBounds {
        x0: rect.left().value,
        y0: rect.bottom().value,
        x1: rect.right().value,
        y1: rect.top().value,
    }
}

/// 回転のない水平glyphでSDKが返す有限の原点だけを、字面矩形と区別して保持する。
pub(crate) fn horizontal_baseline(character: &PdfPageTextChar<'_>) -> Option<f32> {
    let angle = character.angle_degrees().ok()?;
    if !angle.is_finite() || angle.rem_euclid(360.0) != 0.0 {
        return None;
    }
    let y = character.origin_y().ok()?.value;
    y.is_finite().then_some(y)
}
