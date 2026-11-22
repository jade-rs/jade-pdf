//! 原文の一文字コードに対応するRTL展開の、SDKによる文字順の変化を補正する。
//! 同一オブジェクト・同一文字セルの厳密な一致と原文の出現数を必要とする。
use pdfium_render::prelude::Pdfium;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::c_void;
use unicode_normalization::UnicodeNormalization;

use crate::glyph_cell::{Cell, Inventory};
use crate::source_text::unicode::{rtl_key, rtl_letter, Facts};
use crate::unicode_facts::{self, PageFacts};

/// 一文字コード内の順序補正。範囲の両端だけを一ビットずつ保存する。
pub(crate) struct Corrections {
    /// 逆順に読む範囲の先頭。
    starts: Vec<u64>,
    /// 逆順に読む範囲の最後の文字。
    ends: Vec<u64>,
    /// 全体の逆順では表せない、確認済み一セル内の文字位置の置換。
    permutations: Vec<Permutation>,
    /// 二度目の SDK 読み取り時にも同じ文字セルであることを確認する値。
    pub fingerprint: u64,
    /// 原文・セル・回数が一意に対応しなかったことを示す。
    pub requires_review: bool,
}
impl Default for Corrections {
    /// 補正のないページにはビット列を割り当てない。
    fn default() -> Self {
        Self {
            starts: Vec::new(),
            ends: Vec::new(),
            permutations: Vec::new(),
            fingerprint: unicode_facts::FINGERPRINT_START,
            requires_review: false,
        }
    }
}
impl Corrections {
    /// 一意な原文に属する同一セル内だけで、出力位置に対応するSDK文字位置を返す。
    pub fn source_index(&self, index: usize) -> Option<usize> {
        let at = self.permutations.partition_point(|p| p.start <= index);
        if let Some(p) = at.checked_sub(1).and_then(|at| self.permutations.get(at)) {
            if let Some(&offset) = p.order.get(index - p.start) {
                return Some(p.start + usize::from(offset));
            }
        }
        self.reverse_index(index)
    }

    /// 最大256文字の同一セル内だけで、対応する元の文字位置を求める。
    fn reverse_index(&self, index: usize) -> Option<usize> {
        if self.starts.is_empty() {
            return None;
        }
        let word = index / 64;
        let mut start = None;
        for offset in 0..=4 {
            let at = word.checked_sub(offset)?;
            let mut bits = *self.starts.get(at)?;
            if offset == 0 {
                bits &= u64::MAX >> (63 - index % 64);
            }
            if bits != 0 {
                start = Some(at * 64 + 63 - bits.leading_zeros() as usize);
                break;
            }
        }
        let start = start?;
        if index - start >= 256 {
            return None;
        }
        for offset in 0..=4 {
            let at = start / 64 + offset;
            let mut bits = *self.ends.get(at)?;
            if offset == 0 {
                bits &= u64::MAX << (start % 64);
            }
            if bits != 0 {
                let end = at * 64 + bits.trailing_zeros() as usize;
                return (index <= end && end - start < 256).then(|| start + end - index);
            }
        }
        None
    }
    /// 同じ文字数を持つ一セル内の逆順だけを登録する。
    fn insert(&mut self, start: usize, end: usize, characters: usize) {
        if self.starts.is_empty() {
            self.starts.resize(characters.div_ceil(64), 0);
            self.ends.resize(characters.div_ceil(64), 0);
        }
        self.starts[start / 64] |= 1 << (start % 64);
        self.ends[end / 64] |= 1 << (end % 64);
    }
}

/// 同じ字形セルを共有する文字だけの、最大256個の全単射。
struct Permutation {
    /// 対象となる連続SDK文字位置の先頭。
    start: usize,
    /// 原文の各文字に対応する、セル内の元の位置。重複文字は出現順を保持する。
    order: Vec<u8>,
}

/// 正規化で文字を増減せず、一文字ずつ完全に対応する場合だけ置換を組み立てる。
fn permutation(start: usize, actual: &str, expected: &str) -> Option<Permutation> {
    let count = actual.chars().count();
    if !(2..=256).contains(&count) || expected.chars().count() != count {
        return None;
    }
    let mut positions: BTreeMap<char, VecDeque<u8>> = BTreeMap::new();
    for (index, c) in actual.chars().enumerate() {
        positions.entry(c).or_default().push_back(index as u8);
    }
    let order: Option<Vec<_>> = expected
        .chars()
        .map(|c| positions.get_mut(&c)?.pop_front())
        .collect();
    Some(Permutation {
        start,
        order: order?,
    })
}

/// 連続する同一セルの文字。256文字を超えた内容は候補として保存しない。
struct Group {
    /// オブジェクト・矩形・原点の厳密な一致。
    cell: Cell,
    /// 元のSDK文字位置。
    start: usize,
    /// 元のSDK文字位置の末尾。
    end: usize,
    /// グループ内の文字列。
    text: String,
    /// グループ内のUnicode scalar数。
    count: usize,
}
/// 原文の候補と一致したセルの回数と補正範囲。
#[derive(Default)]
struct Matches {
    /// 同じ文字集合を持つ全セルの個数。
    count: usize,
    /// 元の文字列を逆順にした場合だけ正解する範囲。
    reversals: Vec<(usize, usize)>,
    /// 原文と文字が一対一に一致する、全体逆順以外の並べ替え。
    permutations: Vec<Permutation>,
    /// 正順・逆順のどちらでも原文と一致しない場合。
    ambiguous: bool,
}

/// 使用された原文の対応表と一致するセルだけを数える。
fn finish(group: Group, source: &Facts, matches: &mut BTreeMap<String, Matches>) {
    if group.count < 2 || group.count > 256 || !group.text.chars().any(rtl_letter) {
        return;
    }
    let normalized: String = group.text.nfd().collect();
    let key = rtl_key(&normalized);
    let Some(sequence) = source.rtl_sequences.get(&key) else {
        return;
    };
    let found = matches.entry(key).or_default();
    found.count += 1;
    match &sequence.text {
        Some(expected) if expected == &normalized => {}
        Some(expected)
            if group
                .text
                .chars()
                .rev()
                .collect::<String>()
                .nfd()
                .eq(expected.chars()) =>
        {
            found.reversals.push((group.start, group.end))
        }
        Some(expected) => {
            if let Some(order) = permutation(group.start, &group.text, expected) {
                found.permutations.push(order);
            } else {
                found.ambiguous = true;
            }
        }
        None => found.ambiguous = true,
    }
}

/// 原文の文字コード数・Unicode集合・セル個数・同じ語の出現数を照合する。
pub(crate) fn read(
    pdfium: &Pdfium,
    text: *mut c_void,
    facts: &PageFacts,
    source: Option<&Facts>,
) -> Corrections {
    let mut output = Corrections::default();
    let Some(source) = source else {
        return output;
    };
    if !source.rtl_unavailable && !source.rtl_sequences.values().any(|s| s.explicit) {
        return output;
    }
    output.requires_review = true;
    if source.rtl_unavailable
        || source.unavailable
        || source.glyph_ownership_unavailable
        || facts.errors != 0
        || facts.source_text_characters_missing != 0
        || facts.source_unicode_scalars_unmatched != 0
        || facts.source_text_accounting_unavailable
        || facts.sdk_text_objects_without_characters != 0
        || !facts.actual_regions.is_empty()
    {
        return output;
    }
    let b = pdfium.bindings();
    let mut group: Option<Group> = None;
    let mut cells = Inventory::default();
    let mut matches = BTreeMap::new();
    for index in 0..facts.characters {
        if facts.projected(index) {
            return output;
        }
        // 補正のビット位置はSDKの文字位置なので、UTF-16対を跨ぐ逆順は確定しない。
        if facts.is_paired_low(index + 1) {
            return output;
        }
        if facts.is_paired_low(index) {
            continue;
        }
        let generated = b.FPDFText_IsGenerated(text.cast(), index as i32);
        if generated == 1 {
            if let Some(old) = group.take() {
                finish(old, source, &mut matches);
            }
            continue;
        }
        if generated != 0 {
            return output;
        }
        let Some(cell) = Cell::read(pdfium, text, index as i32) else {
            return output;
        };
        let raw = b.FPDFText_GetUnicode(text.cast(), index as i32);
        let scalar = if facts.is_paired_low(index + 1) {
            unicode_facts::surrogate_scalar(
                raw,
                b.FPDFText_GetUnicode(text.cast(), index as i32 + 1),
            )
        } else {
            char::from_u32(raw)
        };
        let Some(scalar) = scalar else {
            return output;
        };
        if group.as_ref().is_none_or(|g| g.cell != cell) {
            if let Some(old) = group.take() {
                finish(old, source, &mut matches);
            }
            if cells.insert(cell).is_none() {
                return output;
            }
            group = Some(Group {
                cell,
                start: index,
                end: index,
                text: String::new(),
                count: 0,
            });
        }
        let g = group.as_mut().unwrap();
        g.end = index;
        g.count += 1;
        if g.count <= 256 {
            g.text.push(scalar);
        }
    }
    if let Some(old) = group {
        finish(old, source, &mut matches);
    }
    // SDK の一文字コードは一つのセルにしか対応しない。欠落や重なりでセルが
    // 合流した場合は、文字集合が偶然一致しても補正しない。
    if cells.len() != source.glyphs {
        return output;
    }
    let mut ranges = Vec::new();
    let mut permutations = Vec::new();
    for (key, expected) in &source.rtl_sequences {
        let found = matches.get(key);
        if expected.text.is_none() || found.is_none_or(|m| m.ambiguous || m.count != expected.count)
        {
            return output;
        }
        let found = matches.remove(key).unwrap();
        ranges.extend(found.reversals);
        permutations.extend(found.permutations);
    }
    output.requires_review = false;
    permutations.sort_unstable_by_key(|p| p.start);
    output.permutations = permutations;
    for (start, end) in ranges {
        output.insert(start, end, facts.characters);
    }
    for index in 0..facts.characters {
        if output.source_index(index).is_some() {
            output.fingerprint =
                unicode_facts::raw_geometry(pdfium, text, index as i32, output.fingerprint);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// 重複を含む文字の並べ替えも、各SDK位置を一度だけ使い原文を過不足なく再現する。
        #[test]
        fn source_permutation_is_a_bijection(actual in prop::collection::vec(any::<char>(), 2..257), shift in 0usize..256) {
            let count = actual.len();
            let mut expected = actual.clone();
            expected.rotate_left(shift % count);
            let raw: String = actual.iter().collect();
            let target: String = expected.iter().collect();
            let order = permutation(17, &raw, &target).unwrap();
            let restored: Vec<_> = order.order.iter().map(|&at| actual[usize::from(at)]).collect();
            prop_assert_eq!(restored, expected);
            let mut used = order.order.clone();
            used.sort_unstable();
            prop_assert_eq!(used.iter().map(|&at| usize::from(at)).collect::<Vec<_>>(), (0..count).collect::<Vec<_>>());
            let corrections = Corrections { permutations: vec![order], ..Corrections::default() };
            prop_assert_eq!(corrections.source_index(16), None);
            prop_assert_eq!(corrections.source_index(17 + count), None);
            for index in 0..count {
                prop_assert_eq!(actual[corrections.source_index(17 + index).unwrap() - 17], target.chars().nth(index).unwrap());
            }
        }

        /// ビット境界を跨いでも、同じセルの範囲外へ文字を移動しない。
        #[test]
        fn reversal_is_an_involution_with_exact_boundaries(start in 0usize..4096, length in 2usize..257) {
            let end = start + length - 1;
            let mut corrections = Corrections::default();
            corrections.insert(start, end, end + 80);
            for index in start..=end {
                let other = corrections.reverse_index(index).unwrap();
                prop_assert_eq!(other, start + end - index);
                prop_assert_eq!(corrections.reverse_index(other), Some(index));
            }
            if start > 0 { prop_assert_eq!(corrections.reverse_index(start - 1), None); }
            prop_assert_eq!(corrections.reverse_index(end + 1), None);
        }
    }

    /// 文字の欠落・追加・置換や正規化による個数差を、並べ替えだけで修復したと扱わない。
    #[test]
    fn permutation_requires_exact_literal_scalars_and_a_bounded_cell() {
        for (actual, expected) in [
            ("بABت", "بACت"),
            ("بABت", "بAت"),
            ("بABت", "بABBت"),
            ("بéت", "بe\u{301}ت"),
            ("ب", "ب"),
        ] {
            assert!(permutation(0, actual, expected).is_none());
        }
        assert!(permutation(0, &"ب".repeat(257), &"ب".repeat(257)).is_none());
        let order = permutation(0, "ت12ب12", "ب12ت12").unwrap();
        assert_eq!(order.order, [3, 1, 2, 0, 4, 5]);
    }

    /// 隣接する複数のセルを、それぞれ独立した範囲として扱う。
    #[test]
    fn adjacent_and_separated_cells_never_share_characters() {
        let mut corrections = Corrections::default();
        for (start, end) in [(0, 1), (2, 4), (63, 65), (127, 382)] {
            corrections.insert(start, end, 450);
        }
        for index in 0..450 {
            let range = [(0, 1), (2, 4), (63, 65), (127, 382)]
                .into_iter()
                .find(|(a, b)| *a <= index && index <= *b);
            assert_eq!(
                corrections.reverse_index(index),
                range.map(|(a, b)| a + b - index)
            );
        }
    }
}
