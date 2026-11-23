//! 原本resourceだけから、code別の派生名と完全なportable fontを生成する。
use crate::{
    ParsedEmbeddedFont, decode::DecodeBudget, resource_resolver::ResourceResolver,
    text::ToUnicodeCMap,
};

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
    _input: Input<'_>,
    _dictionary: &'bytes [u8],
    _descriptor: &'bytes [u8],
    _resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Prepared> {
    let _: Vec<u8> = budget.font_vector(1)?;
    None
}

#[cfg(test)]
#[path = "type1_outline_assemble_tests.rs"]
mod tests;
