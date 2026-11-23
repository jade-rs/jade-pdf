//! 原本glyphを保ちながら、PDF codeごとに異なる幅とUnicode列を出力計画へ残す。
use super::{file::Font, unicode::Mapping};
use crate::decode::DecodeBudget;

/// 元glyphとその幅違いを含む、出力GID順の計画。
pub(super) struct Plan {
    /// 全原本glyphを先頭に保ち、必要な幅違いだけを後ろへ追加する。
    pub(super) glyphs: Vec<Glyph>,
    /// 元の256code順に保持する、Unicodeと外形の対応。
    pub(super) codes: Vec<Code>,
}

/// 元の外形を複製せず参照する、一つの出力glyph。
pub(super) struct Glyph {
    /// 原本FontFile内のGID。外形とglyph名は原本に由来する。
    pub(super) original: u16,
    /// PDFが上書きする幅。Noneは元programのadvanceを保つ。
    pub(super) pdf_width: Option<f64>,
}

/// Unicode文字列と描画glyphを同一視しない、一つのPDF code。
pub(super) struct Code {
    /// 空文字列・未知文字・明示的ToUnicodeと元の幅をそのまま保つ。
    pub(super) mapping: Mapping,
    /// 確認済みの非.notdef出力GID。欠落と.notdefの区別はmapping内に残す。
    pub(super) glyph: Option<u16>,
}

/// codeをUnicodeやglyph名から推測せず、幅違いと複数scalarを計画する。
pub(super) fn build(
    budget: &DecodeBudget,
    font: &Font<'_>,
    mappings: Vec<Mapping>,
) -> Option<Plan> {
    if mappings.len() != 256
        || font.glyphs.is_empty()
        || font.glyphs.len() > 4096
        || font.glyphs[0].name != b".notdef"
        || budget.is_stopped()
    {
        return None;
    }
    for glyph in &font.glyphs {
        if budget.is_stopped() || !glyph.outline.advance.is_finite() || glyph.outline.advance < 0. {
            return None;
        }
    }
    let mut count = font.glyphs.len();
    for (index, mapping) in mappings.iter().enumerate() {
        let source = mapping.source;
        if budget.is_stopped()
            || !source.width.is_finite()
            || source.width < 0.
            || source
                .glyph
                .is_some_and(|g| usize::from(g) >= font.glyphs.len())
        {
            return None;
        }
        if let Some(scalars) = &mapping.scalars {
            if scalars.len() > 65535 {
                return None;
            }
            for &scalar in scalars {
                if budget.is_stopped() || char::from_u32(scalar).is_none() {
                    return None;
                }
            }
        }
        if let Some(original) = source.glyph.filter(|&g| g != 0)
            && source.width != font.glyphs[usize::from(original)].outline.advance
            && !mappings[..index].iter().any(|prior| {
                prior.source.glyph == source.glyph && prior.source.width == source.width
            })
        {
            count += 1;
            if count > 4096 {
                return None;
            }
        }
    }
    let mut glyphs = budget.font_vector(count)?;
    let mut codes = budget.font_vector(256)?;
    for original in 0..font.glyphs.len() {
        if budget.is_stopped() {
            return None;
        }
        glyphs.push(Glyph {
            original: original as u16,
            pdf_width: None,
        });
    }
    for mapping in mappings {
        if budget.is_stopped() {
            return None;
        }
        let glyph = match mapping.source.glyph.filter(|&g| g != 0) {
            None => None,
            Some(original)
                if mapping.source.width == font.glyphs[usize::from(original)].outline.advance =>
            {
                Some(original)
            }
            Some(original) => {
                let index = glyphs
                    .iter()
                    .position(|entry| {
                        entry.original == original && entry.pdf_width == Some(mapping.source.width)
                    })
                    .unwrap_or_else(|| {
                        let index = glyphs.len();
                        glyphs.push(Glyph {
                            original,
                            pdf_width: Some(mapping.source.width),
                        });
                        index
                    });
                Some(index as u16)
            }
        };
        codes.push(Code { mapping, glyph });
    }
    (!budget.is_stopped()).then_some(Plan { glyphs, codes })
}

#[cfg(test)]
#[path = "type1_outline_plan_tests.rs"]
mod tests;
