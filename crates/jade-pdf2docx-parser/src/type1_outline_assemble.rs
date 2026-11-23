//! 原本resourceだけから、code別の派生名と完全なportable fontを生成する。
use super::{
    attributes, cff, file, gsub, identity, opentype, permission, plan, resource, routes, unicode,
};
use crate::{
    ParsedEmbeddedFont, decode::DecodeBudget, resource_resolver::ResourceResolver,
    text::ToUnicodeCMap,
};
#[path = "type1_outline_assemble_glyphs.rs"]
mod glyphs;

/// 未変更の元FontFileと、同じresourceの原文対応。
pub(crate) struct Input<'a> {
    /// 展開済みの元FontFile byte列。
    pub(crate) program: &'a [u8],
    /// 元streamが宣言したcleartext長。
    pub(crate) length1: usize,
    /// 元streamが宣言したeexec長。
    pub(crate) length2: usize,
    /// 同じfont objectの原本ToUnicode。
    pub(crate) cmap: &'a ToUnicodeCMap,
}

/// 原本一resourceの全出力。途中生成に失敗した場合は公開しない。
pub(crate) struct Prepared {
    /// 派生名・元許諾・原本外形を含む実font。
    pub(crate) fonts: Vec<ParsedEmbeddedFont>,
    /// 元の256code順のfont index。未確認はNone。
    pub(crate) routes: [Option<u16>; 256],
}

impl Prepared {
    /// 原本codeに対応するfontを、UnicodeやBaseFont名から推測せず選ぶ。
    pub(crate) fn family(&self, code: u8) -> Option<&str> {
        Some(
            self.fonts
                .get(usize::from(self.routes[usize::from(code)]?))?
                .font_name
                .as_str(),
        )
    }
}

/// 復号・権利・属性・幅・文字列・元clusterを、同じ入力から一貫して生成する。
pub(crate) fn prepare<'bytes>(
    budget: &DecodeBudget,
    input: Input<'_>,
    dictionary: &'bytes [u8],
    descriptor: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Prepared> {
    let font = file::read(budget, input.program, input.length1, input.length2)?;
    let attributes = attributes::read(budget, &font, descriptor, resolver)?;
    let base = resource::values::field(budget, dictionary, b"BaseFont", resolver)?;
    let subset = subset_name(font.header.name)
        || base
            .and_then(resource::values::name)
            .is_some_and(subset_name);
    let permission::Decision::Allowed(license) = permission::read(
        budget,
        input.program.get(..input.length1)?,
        &font.header.info,
        subset,
    )?
    else {
        return None;
    };
    let slots = resource::read(budget, &font, dictionary, resolver)?;
    let mapping = unicode::map(budget, &font, &slots, input.cmap)?;
    let plan = plan::build(budget, &font, mapping)?;
    let routes = routes::build(budget, &plan)?;
    if routes.faces.is_empty() {
        return None;
    }
    let data = glyphs::build(budget, &font, &plan, &attributes)?;
    let mut entries = budget.font_vector(data.programs.len())?;
    for (name, program) in data.names.iter().zip(&data.programs) {
        entries.push(cff::Entry { name, program });
    }
    let output = cff::Font {
        units_per_em: data.units,
        name: b"JadeT1-Source",
        notice: &license.notice,
        bbox: data.bbox,
        italic_angle: attributes.italic_angle,
        is_fixed_pitch: attributes.fixed,
        glyphs: &entries,
    };
    let identity = identity::Source {
        program: input.program,
        plan: &plan,
    };
    let mut fonts = budget.metadata_vector(routes.faces.len())?;
    for face in routes.faces {
        if budget.is_stopped() {
            return None;
        }
        let info = attributes.to_info(data.units, &data.metrics, &face.mappings, &license)?;
        let rule = face.ligature.map(|glyph| gsub::Rule {
            components: &face.components,
            glyph,
        });
        let named = opentype::encode_derived(budget, &identity, &output, &info, rule.as_slice())?;
        fonts.push(ParsedEmbeddedFont {
            font_name: named.family,
            bytes: named.bytes,
        });
    }
    (!budget.is_stopped()).then_some(Prepared {
        fonts,
        routes: routes.codes,
    })
}

/// PDFの正式な六大文字prefixだけを、既にsubsetである指定として扱う。
fn subset_name(name: &[u8]) -> bool {
    name.len() > 7 && name[6] == b'+' && name[..6].iter().all(u8::is_ascii_uppercase)
}

#[cfg(test)]
#[path = "type1_outline_assemble_tests.rs"]
mod tests;
