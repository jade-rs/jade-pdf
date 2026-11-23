//! 元の外形を、hintやPostScriptを含まない有限のType2 programへ格納する。
use super::charstring::Glyph;
use crate::decode::DecodeBudget;

/// u16の全advanceをType2の符号付き範囲へ収める、Private DICT共通の基準幅。
pub(super) const NOMINAL_WIDTH: i32 = 32768;

/// 絶対座標を一度だけ固定小数点へ丸め、保存前に全byteを累積予算へ予約する。
pub(super) fn encode(budget: &DecodeBudget, glyph: &Glyph) -> Option<Vec<u8>> {
    let _ = (budget, glyph);
    None
}

#[cfg(test)]
#[path = "type1_outline_type2_tests.rs"]
mod tests;
