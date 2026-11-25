//! SDK専用の識別候補を、元のsubset Type1/CFF辞書から保守的に確認する。
use crate::{decode::DecodeBudget, resource_resolver::ResourceResolver};

#[cfg(test)]
#[path = "pdfium_subset_weight_tests.rs"]
mod tests;

/// 辞書から確認した候補。実際の採用にはSDKの埋め込み確認を別途必要とする。
pub(crate) struct Candidate<'a> {
    /// 埋め込みが成立した場合にSDKが返す、subset prefixを除いたASCII名。
    pub(crate) native_name: &'a str,
    /// 明示された整数のFontDescriptor weight。
    pub(crate) weight: u16,
}

/// sourceの一意なsubset名・descriptor・Type1C streamだけを候補として読む。
pub(crate) fn read<'a>(
    _resolver: &mut ResourceResolver<'_, 'a>,
    _font: &'a [u8],
    _budget: &DecodeBudget,
) -> Option<Candidate<'a>> {
    None
}
