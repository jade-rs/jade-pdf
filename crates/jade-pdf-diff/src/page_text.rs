//! 元の行データを保持した、ページの抽出順に沿う本文差分。
use crate::{diff::segment_text, ir::normalize, BlockIr, Segment, SegmentKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

/// ページの抽出済み本文を、共通のブロックで区切って比較した結果。
/// 段落・表の意味や描画の同一性は推定せず、元の読み順の診断を保持する。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageTextDiff {
    /// 折り返しを空白として正規化した本文の、変更がある連続範囲。
    pub groups: Vec<PageTextChangeGroup>,
    /// 細かな対応付けがページ全体の計算予算に制限されたか。
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub alignment_limited: bool,
}

/// 原文の連続ブロック範囲と、すべての文字を再構成できる差分。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageTextChangeGroup {
    /// 旧PageIr.blocks内の0始まり・終端を含まない範囲。追加だけなら空の範囲。
    pub old_block_range: [usize; 2],
    /// 新PageIr.blocks内の0始まり・終端を含まない範囲。削除だけなら空の範囲。
    pub new_block_range: [usize; 2],
    /// 旧範囲の各ブロックが持つ元のindex。行差分・座標への参照を保持する。
    pub old_blocks: Vec<usize>,
    /// 新範囲の各ブロックが持つ元のindex。行差分・座標への参照を保持する。
    pub new_blocks: Vec<usize>,
    /// 旧範囲の全文。既存IRと同じ正規化を適用する。
    pub old_text: String,
    /// 新範囲の全文。行の境界も一つの空白として扱う。
    pub new_text: String,
    /// 共通部分を含む書記素単位の内訳。両側の全文を再構成できる。
    pub segments: Vec<Segment>,
}

/// 連続範囲の全ブロックを順序通りに連結し、文字を省略せず正規化する。
fn text(blocks: &[BlockIr]) -> String {
    normalize(
        &blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// 参照元の寿命を保持し、同じブロック本文の重複回数を数える。
fn counts(blocks: &[BlockIr]) -> HashMap<&str, usize> {
    let mut out = HashMap::new();
    for b in blocks {
        *out.entry(b.text.as_str()).or_insert(0usize) += 1;
    }
    out
}

/// 同じ本文が一度だけ現れる対応から、交差しない最長の共通アンカー列を選ぶ。
fn anchors(old: &[BlockIr], new: &[BlockIr], matched: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let old_counts = counts(old);
    let new_counts = counts(new);
    let mut candidates: Vec<_> = matched
        .iter()
        .copied()
        .filter(|&(o, n)| {
            old.get(o).zip(new.get(n)).is_some_and(|(a, b)| {
                a.text == b.text
                    && old_counts.get(a.text.as_str()) == Some(&1)
                    && new_counts.get(b.text.as_str()) == Some(&1)
            })
        })
        .collect();
    candidates.sort_unstable();
    candidates.dedup();
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![None; candidates.len()];
    for (at, &(_, n)) in candidates.iter().enumerate() {
        let slot = tails.partition_point(|&i| candidates[i].1 < n);
        previous[at] = slot.checked_sub(1).map(|i| tails[i]);
        if slot == tails.len() {
            tails.push(at)
        } else {
            tails[slot] = at
        }
    }
    let mut out = Vec::new();
    let mut at = tails.last().copied();
    while let Some(i) = at {
        out.push(candidates[i]);
        at = previous[i];
    }
    out.reverse();
    out
}

/// 共通の前後を除いた書記素対の総数を制限し、超過時も全文を粗い内訳として保持する。
fn segments(old: &str, new: &str, remaining: &mut usize) -> (Vec<Segment>, bool) {
    let a: Vec<_> = old.graphemes(true).collect();
    let b: Vec<_> = new.graphemes(true).collect();
    let prefix = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let a_end = a.len() - suffix;
    let b_end = b.len() - suffix;
    let work = (a_end - prefix).saturating_mul(b_end - prefix);
    if work <= *remaining {
        *remaining -= work;
        return (segment_text(old, new), false);
    }
    let mut out = Vec::new();
    for (kind, parts) in [
        (SegmentKind::Equal, &a[..prefix]),
        (SegmentKind::Delete, &a[prefix..a_end]),
        (SegmentKind::Insert, &b[prefix..b_end]),
        (SegmentKind::Equal, &a[a_end..]),
    ] {
        if !parts.is_empty() {
            out.push(Segment {
                kind,
                text: parts.concat(),
            });
        }
    }
    (out, true)
}

/// 対応したページの全ブロックを、未比較の文字を残さず連続区間として扱う。
pub(crate) fn compare(
    old: &[BlockIr],
    new: &[BlockIr],
    matched: &[(usize, usize)],
) -> PageTextDiff {
    let mut out = PageTextDiff::default();
    if text(old) == text(new) {
        return out;
    }
    let mut remaining = diff_support::MAX_LCS_CELLS;
    let mut before = (0, 0);
    let mut boundaries = anchors(old, new, matched);
    boundaries.push((old.len(), new.len()));
    for (o, n) in boundaries {
        let a = text(&old[before.0..o]);
        let b = text(&new[before.1..n]);
        if a != b {
            let (segments, limited) = segments(&a, &b, &mut remaining);
            out.alignment_limited |= limited;
            out.groups.push(PageTextChangeGroup {
                old_block_range: [before.0, o],
                new_block_range: [before.1, n],
                old_blocks: old[before.0..o].iter().map(|b| b.index).collect(),
                new_blocks: new[before.1..n].iter().map(|b| b.index).collect(),
                old_text: a,
                new_text: b,
                segments,
            });
        }
        before = (o.saturating_add(1), n.saturating_add(1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 連続範囲を比べても予算を使い直さず、超過時も書記素と全文を欠落させない。
    #[test]
    fn aggregate_budget_and_grapheme_reconstruction_survive_fallback() {
        let mut remaining = 4;
        let (_, limited) = segments("ab", "cd", &mut remaining);
        assert!(!limited);
        assert_eq!(remaining, 0);
        let old = "a👩‍💻b";
        let new = "c👩‍💻d";
        let (parts, limited) = segments(old, new, &mut remaining);
        assert!(limited);
        let before: String = parts
            .iter()
            .filter(|s| s.kind != SegmentKind::Insert)
            .map(|s| s.text.as_str())
            .collect();
        let after: String = parts
            .iter()
            .filter(|s| s.kind != SegmentKind::Delete)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(before, old);
        assert_eq!(after, new);
        assert!(parts
            .iter()
            .filter(|s| s.text.contains('👩'))
            .all(|s| s.text.contains("👩‍💻")));
    }
}
