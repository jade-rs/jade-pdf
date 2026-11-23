//! 確認済みPDF code/GID/幅へ、原本ToUnicodeを優先した文字列を結び付ける。
use super::{file::Font, resource::Slot};
use crate::{decode::DecodeBudget, text::ToUnicodeCMap};

/// codeごとの元の輪郭・幅と、正規化していないUnicode列。
pub(super) struct Mapping {
    /// 元resourceの対応を、未知glyphや.notdefも含めて保持する。
    pub(super) source: Slot,
    /// 未確認はNone。明示的な空文字列はSome(empty)として区別する。
    pub(super) scalars: Option<Vec<u32>>,
    /// 元ToUnicodeの明示対応を採用したか。glyph名からの補完とは区別する。
    pub(super) from_tounicode: bool,
}

/// code番号からUnicodeを推測せず、原本の文字列と全codeの対応を返す。
pub(super) fn map(
    budget: &DecodeBudget,
    font: &Font<'_>,
    slots: &[Slot],
    original: &ToUnicodeCMap,
) -> Option<Vec<Mapping>> {
    if slots.len() != 256
        || font.glyphs.is_empty()
        || budget.is_stopped()
        || slots.iter().any(|s| {
            !s.width.is_finite() || s.glyph.is_some_and(|g| usize::from(g) >= font.glyphs.len())
        })
    {
        return None;
    }
    let mut mappings = budget.font_vector(slots.len())?;
    for (code, &source) in slots.iter().enumerate() {
        if budget.is_stopped() {
            return None;
        }
        let explicit = original.lookup_byte(code as u8);
        let scalars = match explicit {
            Some(text) => {
                let count = text.chars().count();
                if count > 65535 {
                    return None;
                }
                let mut values = budget.font_vector(count)?;
                for scalar in text.chars() {
                    if budget.is_stopped() {
                        return None;
                    }
                    values.push(scalar as u32);
                }
                Some(values)
            }
            None => glyph_sequence(budget, font, source.glyph)?,
        };
        mappings.push(Mapping {
            source,
            scalars,
            from_tounicode: explicit.is_some(),
        });
    }
    (!budget.is_stopped()).then_some(mappings)
}

/// 確認済みglyph名だけを二段階で読み、不明componentの部分文字列を公開しない。
fn glyph_sequence(
    budget: &DecodeBudget,
    font: &Font<'_>,
    glyph: Option<u16>,
) -> Option<Option<Vec<u32>>> {
    let Some(glyph) = glyph.filter(|g| *g != 0) else {
        return Some(None);
    };
    let name = &font.glyphs[usize::from(glyph)].name;
    let mut count = 0usize;
    let known = crate::glyph_names::visit(budget, name, false, |_| {
        count = count.checked_add(1)?;
        Some(())
    })?;
    if !known || count == 0 {
        return Some(None);
    }
    if count > 65535 {
        return None;
    }
    let mut values = budget.font_vector(count)?;
    let known = crate::glyph_names::visit(budget, name, false, |scalar| {
        values.push(scalar as u32);
        Some(())
    })?;
    if !known || budget.is_stopped() {
        None
    } else {
        Some(Some(values))
    }
}

#[cfg(test)]
#[path = "type1_outline_unicode_tests.rs"]
mod tests;
