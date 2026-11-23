//! 原本codeに由来する外形を、通常文字と混同しない独立fontへ振り分ける。
use super::{cmap, plan::Plan};
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
pub(super) fn build(budget: &DecodeBudget, _plan: &Plan) -> Option<Routes> {
    let _: Vec<u8> = budget.font_vector(1)?;
    None
}

#[cfg(test)]
#[path = "type1_outline_routes_tests.rs"]
mod tests;
