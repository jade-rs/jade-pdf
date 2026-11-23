//! 原本codeに由来する外形を、通常文字と混同しない独立fontへ振り分ける。
use super::{
    cmap,
    plan::{Code, Plan},
};
use crate::decode::DecodeBudget;

/// 各codeの出力先と、そのfontが持つcmap・原本合字。
pub(super) struct Routes {
    /// 参照された出力先。通常文字のfontにはGSUBを付けない。
    pub(super) faces: Vec<Face>,
    /// 原本code順のfont index。未確認の対応はNoneのまま保つ。
    pub(super) codes: [Option<u16>; 256],
}

/// 一つのfontへ同時に置いてよい文字と、単一の元cluster。
pub(super) struct Face {
    /// このfontで実際に使うUnicodeと出力GIDの対応。
    pub(super) mappings: Vec<cmap::Mapping>,
    /// required合字の入力GID列。通常fontと単独scalarは空。
    pub(super) components: Vec<u16>,
    /// 合字後の元出力GID。NoneのfontにはGSUBを適用しない。
    pub(super) ligature: Option<u16>,
}

/// 一文字の競合とASCII英字合字を分離し、未対応clusterを推測で補わない。
pub(super) fn build(budget: &DecodeBudget, plan: &Plan) -> Option<Routes> {
    if budget.is_stopped()
        || plan.codes.len() != 256
        || plan.glyphs.is_empty()
        || plan.glyphs.len() > 4096
    {
        return None;
    }
    let mut singles = [None; 256];
    let mut single_count = 0;
    for code in &plan.codes {
        if budget.is_stopped()
            || code
                .glyph
                .is_some_and(|g| g == 0 || usize::from(g) >= plan.glyphs.len())
        {
            return None;
        }
        if let Some(scalars) = &code.mapping.scalars {
            if scalars.len() > 65535 {
                return None;
            }
            for &scalar in scalars {
                if budget.is_stopped() || char::from_u32(scalar).is_none() {
                    return None;
                }
            }
        }
        if let Some((&[scalar], glyph)) = candidate(code)
            && base(&singles[..single_count], scalar).is_none()
        {
            singles[single_count] = Some(cmap::Mapping { scalar, glyph });
            single_count += 1;
        }
    }
    let base_count = usize::from(single_count != 0);
    let mut codes = [None; 256];
    let mut leaders = [0usize; 256];
    let mut variant_count = 0;
    for (index, code) in plan.codes.iter().enumerate() {
        if budget.is_stopped() {
            return None;
        }
        let Some((scalars, glyph)) = candidate(code) else {
            continue;
        };
        if let &[scalar] = scalars {
            if base(&singles[..single_count], scalar) == Some(glyph) {
                codes[index] = Some(0);
                continue;
            }
        } else if !(2..=32).contains(&scalars.len())
            || scalars.iter().any(|&scalar| {
                !matches!(scalar,65..=90 | 97..=122)
                    || base(&singles[..single_count], scalar).is_none()
            })
        {
            // Script依存の並替えやdefault ignorableは、確認済み英字合字と同一扱いしない。
            continue;
        }
        let variant = leaders[..variant_count]
            .iter()
            .position(|&prior| candidate(&plan.codes[prior]) == Some((scalars, glyph)));
        let variant = variant.unwrap_or_else(|| {
            let result = variant_count;
            leaders[result] = index;
            variant_count += 1;
            result
        });
        codes[index] = Some(u16::try_from(base_count + variant).ok()?);
    }
    let mut faces = budget.font_vector(base_count + variant_count)?;
    if base_count != 0 {
        let mut mappings = budget.font_vector(single_count)?;
        mappings.extend(singles[..single_count].iter().flatten().copied());
        faces.push(Face {
            mappings,
            components: Vec::new(),
            ligature: None,
        });
    }
    for &index in &leaders[..variant_count] {
        let (scalars, glyph) = candidate(&plan.codes[index])?;
        let unique = scalars
            .iter()
            .enumerate()
            .filter(|(i, s)| !scalars[..*i].contains(s))
            .count();
        let mut mappings = budget.font_vector(unique)?;
        let mut components = if scalars.len() > 1 {
            budget.font_vector(scalars.len())?
        } else {
            Vec::new()
        };
        for (i, &scalar) in scalars.iter().enumerate() {
            if budget.is_stopped() {
                return None;
            }
            let component = if scalars.len() == 1 {
                glyph
            } else {
                base(&singles[..single_count], scalar)?
            };
            if !scalars[..i].contains(&scalar) {
                mappings.push(cmap::Mapping {
                    scalar,
                    glyph: component,
                });
            }
            if scalars.len() > 1 {
                components.push(component);
            }
        }
        faces.push(Face {
            mappings,
            components,
            ligature: (scalars.len() > 1).then_some(glyph),
        });
    }
    (!budget.is_stopped()).then_some(Routes { faces, codes })
}

/// 空・未知の文字列や未確認glyphを、描画可能な対応として返さない。
fn candidate(code: &Code) -> Option<(&[u32], u16)> {
    let scalars = code.mapping.scalars.as_deref().filter(|s| !s.is_empty())?;
    Some((scalars, code.glyph?))
}

/// 元code順で最初に確認した単独scalarの外形だけを、通常fontへ置く。
fn base(singles: &[Option<cmap::Mapping>], scalar: u32) -> Option<u16> {
    singles
        .iter()
        .flatten()
        .find(|m| m.scalar == scalar)
        .map(|m| m.glyph)
}

#[cfg(test)]
#[path = "type1_outline_routes_tests.rs"]
mod tests;
