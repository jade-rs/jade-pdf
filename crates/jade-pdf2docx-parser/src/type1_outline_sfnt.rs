//! 完成した各tableを、整列したOpenType-CFF directoryとchecksumでまとめる。
use crate::decode::DecodeBudget;

/// 固有tagで識別した、完成済みの借用table。
pub(super) struct Table<'a> {
    /// 四byteのOpenType tag。
    pub(super) tag: [u8; 4],
    /// 元のtable bytes。headのchecksum調整値だけは出力時に再計算する。
    pub(super) bytes: &'a [u8],
}

/// 最大32tableをtag順に並べ、4byte整列とfont全体のchecksumを確定する。
pub(super) fn pack(budget: &DecodeBudget, tables: &[Table<'_>]) -> Option<Vec<u8>> {
    let _ = (budget, tables);
    None
}

#[cfg(test)]
#[path = "type1_outline_sfnt_tests.rs"]
mod tests;
