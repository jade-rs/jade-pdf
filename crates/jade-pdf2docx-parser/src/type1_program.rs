//! Type1のcleartextから静的なEncodingを読み、PostScriptを実行しない。
use crate::decode::DecodeBudget;

/// 組み込み表へ適用する、明示的な初期Encoding。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    /// 初期値が.notdefの、256個の独自slot。
    Custom,
    /// Adobe StandardEncodingを元にする。
    Standard,
    /// PostScriptのISOLatin1Encodingを元にする。
    Latin1,
}

/// 入力を借用する256slotと、その初期表。
pub(crate) struct Encoding<'a> {
    /// assignmentのないslotが参照する初期表。
    pub(crate) base: Base,
    /// 後の静的assignmentを適用したglyph名。Noneは初期表を使用する。
    pub(crate) slots: Vec<Option<&'a [u8]>>,
}

/// 有限の静的Encodingのみを解釈し、動的式や破損した表は確認不能として返す。
pub(crate) fn parse<'a>(_budget: &DecodeBudget, _program: &'a [u8]) -> Option<Encoding<'a>> {
    None
}

#[cfg(test)]
#[path = "type1_program_tests.rs"]
mod tests;
