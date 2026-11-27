//! 反復列表が本文から引き取る文字を欠落・重複させない契約。
use super::{ParsedDocument, PositionedTable, text_element_is_in_table};
use std::collections::BTreeMap;

/// 本文の除外と同じ所有判定を使い、表の全文字が一致する場合だけ採用する。
pub(super) fn preserves(parsed: &ParsedDocument, table: &PositionedTable) -> bool {
    let source = parsed
        .text_elements
        .iter()
        .filter(|text| text_element_is_in_table(parsed, text, table))
        .map(|text| text.text.as_str());
    let output = table.table.rows.iter().flatten().map(String::as_str);
    characters(source) == characters(output)
}

/// 断片間の空白補間だけを許し、文字の種類・個数を正規化せず照合する。
fn characters<'a>(parts: impl IntoIterator<Item = &'a str>) -> BTreeMap<char, usize> {
    let mut counts = BTreeMap::new();
    for ch in parts
        .into_iter()
        .flat_map(str::chars)
        .filter(|ch| !ch.is_whitespace())
    {
        *counts.entry(ch).or_default() += 1;
    }
    counts
}
