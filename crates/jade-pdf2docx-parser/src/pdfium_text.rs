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
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let bounds = parsed_bounds_from_pdf_rect(segment.bounds());
        let mut font_size = bounds.y1 - bounds.y0;
        let mut font_name = None;
        let mut fill_color = None;
        if let Some(first) = first_in_rect(characters, segment.bounds()) {
            font_size = first.scaled_font_size().value;
            let name = first.font_name();
            font_name = copy(budget, (!name.is_empty()).then_some(name.as_str()))?;
            let color = first.fill_color().ok().map(|color| color.to_hex());
            fill_color = copy(budget, color.as_deref())?;
        }
        let text = copy(budget, Some(trimmed))?.unwrap_or_default();
        crate::metadata_workspace::push(
            budget,
            &mut elements,
            ParsedTextElement {
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
