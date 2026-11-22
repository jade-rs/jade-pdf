//! 字形IDまで照合した旧Symbol私用符号を、曖昧さのない標準文字へ投影する。
use crate::{
    glyph_cell::{Cell, Inventory},
    source_text::unicode::Facts,
    unicode_facts::{self, PageFacts},
};
use pdfium_render::prelude::Pdfium;
use std::{
    collections::{BTreeMap, HashSet},
    ffi::c_void,
};

/// 原文とセルの個数が一致した文字置換と、復元後の原文集合。
pub(crate) struct Corrections {
    /// 元のSDK位置を昇順に保持する。
    positions: Vec<(usize, char)>,
    /// 二度の読み取りで置換位置の座標を照合する。
    pub fingerprint: u64,
    /// 確認できない旧Symbol符号を成功として扱わない。
    pub requires_review: bool,
    /// 検証済み置換と未確定のまま保持する部品の、多重集合の保存量を証明した。
    pub inventory_verified: bool,
}

impl Default for Corrections {
    /// 対象のないページに文字位置を割り当てない。
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            fingerprint: unicode_facts::FINGERPRINT_START,
            requires_review: false,
            inventory_verified: false,
        }
    }
}

impl Corrections {
    /// 原文で検証したSDK文字位置だけを変更する。
    pub fn get(&self, index: usize) -> Option<char> {
        self.positions
            .binary_search_by_key(&index, |(i, _)| *i)
            .ok()
            .map(|i| self.positions[i].1)
    }
}

/// 未確認の私用文字、RTL、方向制御のあるページで順序を推測しない。
/// 明示的なUnicode 18文脈と、原文から検証済みの旧符号だけを許す。
fn expected(source: &Facts) -> Option<BTreeMap<char, usize>> {
    if source.symbol_aliases.is_empty() || source.unavailable || source.rtl_unavailable {
        return None;
    }
    for (&raw, alias) in &source.symbol_aliases {
        let target = alias.scalar?;
        let component = target == raw && crate::legacy_context::symbol_component_raw(raw);
        if !component && !crate::legacy_context::contains(target) {
            return None;
        }
        if source.scalars.get(&raw) != Some(&alias.count) || alias.count == 0 {
            return None;
        }
    }
    if source
        .scalars
        .keys()
        .any(|&c| !source.symbol_aliases.contains_key(&c) && !crate::legacy_context::contains(c))
    {
        return None;
    }
    let mut result = source.scalars.clone();
    for (&raw, alias) in &source.symbol_aliases {
        result.remove(&raw)?;
        let mut normalized = BTreeMap::new();
        crate::source_text::unicode::add_scalar(&mut normalized, alias.scalar?);
        for (c, count) in normalized {
            let count = count.checked_mul(alias.count)?;
            let old = result.entry(c).or_default();
            *old = old.checked_add(count)?;
        }
    }
    Some(result)
}

/// 個々の私用文字の所有セルとページ全体のUnicode多重集合を確認する。
/// 無関係なフォントの空白欠落や既存のSDK診断を消すことはない。
pub(crate) fn read(
    pdfium: &Pdfium,
    text: *mut c_void,
    facts: &PageFacts,
    source: Option<&Facts>,
    observed: &BTreeMap<char, usize>,
) -> Corrections {
    let Some(source) = source else {
        return Corrections::default();
    };
    let mut output = Corrections {
        requires_review: source.symbol_private,
        ..Corrections::default()
    };
    if !source.symbol_private || facts.errors != 0 || facts.source_unicode_accounting_unavailable {
        return output;
    }
    let Some(declared) = expected(source) else {
        return output;
    };
    if *observed == declared {
        output.inventory_verified = true;
        output.requires_review = has_opaque_components(source);
        return output;
    }
    if *observed != source.scalars {
        return output;
    }
    let b = pdfium.bindings();
    let mut cells = Inventory::default();
    let mut candidates = HashSet::new();
    let mut counts = BTreeMap::<char, usize>::new();
    let mut positions = Vec::new();
    for index in 0..facts.characters {
        if facts.projected(index) {
            continue;
        }
        if facts.is_paired_low(index) {
            continue;
        }
        match b.FPDFText_IsGenerated(text.cast(), index as i32) {
            1 => continue,
            0 => {}
            _ => return output,
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
        let alias = source.symbol_aliases.get(&scalar);
        let Some(fresh) = cells.insert(cell) else {
            return output;
        };
        // 同じセルの展開文字が前後に離れていても、一文字の所有証明に混ぜない。
        if !fresh && (alias.is_some() || candidates.contains(&cell)) {
            return output;
        }
        if let Some(alias) = alias {
            if facts.rtl_order.reverse_index(index).is_some()
                || facts.supplementary.get(index).is_some()
            {
                return output;
            }
            candidates.insert(cell);
            *counts.entry(scalar).or_default() += 1;
            if alias.scalar != Some(scalar) {
                positions.push((index, alias.scalar.unwrap()));
            }
        }
    }
    if source
        .symbol_aliases
        .iter()
        .any(|(c, alias)| counts.get(c) != Some(&alias.count))
    {
        return output;
    }
    let mut fingerprint = unicode_facts::FINGERPRINT_START;
    for (index, _) in &positions {
        fingerprint = unicode_facts::raw_geometry(pdfium, text, *index as i32, fingerprint);
    }
    output.positions = positions;
    output.fingerprint = fingerprint;
    output.inventory_verified = true;
    output.requires_review = has_opaque_components(source);
    output
}

/// 恒等対応で保持した部品があれば、保存量の照合後も意味の要確認を残す。
fn has_opaque_components(source: &Facts) -> bool {
    source.symbol_aliases.iter().any(|(&raw, alias)| {
        alias.scalar == Some(raw) && crate::legacy_context::symbol_component_raw(raw)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_text::unicode::SymbolAlias;
    use proptest::prelude::*;

    /// 生の原文と、独立に照合した使用回数を持つ検証資料を作る。
    fn source(count: usize) -> Facts {
        let mut source = Facts::default();
        source.symbol_private = true;
        source.scalars.insert('\u{f0a3}', count);
        source.symbol_aliases.insert(
            '\u{f0a3}',
            SymbolAlias {
                scalar: Some('≤'),
                count,
            },
        );
        source
    }

    /// 別フォントで同じ私用符号が使われた場合、字形の欠落とRTLを拒否する。
    #[test]
    fn ambiguous_context_and_unowned_private_scalars_are_not_rewritten() {
        let mut s = source(1);
        assert_eq!(expected(&s), Some(BTreeMap::from([('≤', 1)])));
        for c in ['א', '\u{1e900}', '\u{064b}', '\u{e123}', '\u{0378}'] {
            s.scalars.insert(c, 1);
            assert!(expected(&s).is_none());
            s.scalars.remove(&c);
        }
        s.scalars.insert('\u{f0a3}', 2);
        assert!(expected(&s).is_none());
        s.scalars.insert('\u{f0a3}', 1);
        s.unavailable = true;
        assert!(expected(&s).is_none());
        s.unavailable = false;
        s.symbol_aliases.get_mut(&'\u{f0a3}').unwrap().scalar = None;
        assert!(expected(&s).is_none());
    }

    /// 引用符・アクセント・数学記号のあるLTR本文でも、原文の意味と結合文字を保つ。
    #[test]
    fn assigned_non_rtl_context_preserves_math_and_combining_marks() {
        let mut s = source(1);
        for c in "Café “x” ≥ 3 × 2".chars() {
            crate::source_text::unicode::add_scalar(&mut s.scalars, c);
        }
        let mut wanted = s.scalars.clone();
        wanted.remove(&'\u{f0a3}');
        wanted.insert('≤', 1);
        assert_eq!(expected(&s), Some(wanted));
        s.symbol_aliases.get_mut(&'\u{f0a3}').unwrap().scalar = Some('א');
        assert!(expected(&s).is_none());
    }

    /// 所有を確認した中立の部品は私用のまま保ち、同じページの確定した記号だけを復元する。
    #[test]
    fn owned_opaque_components_allow_known_aliases_without_claiming_their_meaning() {
        let mut s = source(1);
        s.scalars.insert('\u{f0e9}', 2);
        s.symbol_aliases.insert(
            '\u{f0e9}',
            SymbolAlias {
                scalar: Some('\u{f0e9}'),
                count: 2,
            },
        );
        assert_eq!(
            expected(&s),
            Some(BTreeMap::from([('≤', 1), ('\u{f0e9}', 2)]))
        );
        assert!(has_opaque_components(&s));
        s.scalars.insert('\u{f0e9}', 3);
        assert!(
            expected(&s).is_none(),
            "An unowned font or ActualText collision must not borrow the component proof"
        );
        s.scalars.insert('\u{f0e9}', 2);
        s.scalars.insert('\u{e123}', 1);
        s.symbol_aliases.insert(
            '\u{e123}',
            SymbolAlias {
                scalar: Some('\u{e123}'),
                count: 1,
            },
        );
        assert!(
            expected(&s).is_none(),
            "Arbitrary PUA identity aliases are not trusted context"
        );
    }

    proptest! {
        /// 部品の全使用が所有済みの場合だけ既知の記号を復元し、未知の追加使用を拒否する。
        #[test]
        fn opaque_context_requires_exact_source_multiplicity(known in 1usize..1024,owned in 1usize..1024,extra in 0usize..1024) {
            let mut s=source(known);
            s.scalars.insert('\u{f0e9}',owned+extra);
            s.symbol_aliases.insert('\u{f0e9}',SymbolAlias{scalar:Some('\u{f0e9}'),count:owned});
            prop_assert_eq!(expected(&s).is_some(),extra==0);
            prop_assert!(has_opaque_components(&s));
        }

        /// 数の重複を保ち、部分的な証明で未検証の私用文字を巻き込まない。
        #[test]
        fn projection_preserves_exact_multiplicity(count in 1usize..1024, unverified in 1usize..1024) {
            let mut s = source(count);
            prop_assert_eq!(expected(&s), Some(BTreeMap::from([('≤', count)])));
            s.scalars.insert('\u{f0a3}', count + unverified);
            prop_assert!(expected(&s).is_none());
        }
    }
}
