//! 原文の字形名から、Windows SDKで16ビットへ切り詰められた補助文字を復元する。
use crate::glyph_cell::{Cell, Inventory};
use crate::source_text::unicode::Facts;
use crate::unicode_facts::{self, PageFacts};
use pdfium_render::prelude::Pdfium;
use std::collections::BTreeMap;
use std::ffi::c_void;

/// 原文とSDKの保存量を照合した、補助文字の置換位置。
pub(crate) struct Corrections {
    /// 元のSDK文字位置を昇順で保持する。
    positions: Vec<(usize, char)>,
    /// 二度の読み取りで、同じ座標と原点を使ったことを確認する値。
    pub fingerprint: u64,
}

impl Default for Corrections {
    /// 復元のないページには位置配列を割り当てない。
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            fingerprint: unicode_facts::FINGERPRINT_START,
        }
    }
}

impl Corrections {
    /// 検証したSDK文字位置にだけ、原文のUnicodeを返す。
    pub fn get(&self, index: usize) -> Option<char> {
        self.positions
            .binary_search_by_key(&index, |(at, _)| *at)
            .ok()
            .map(|at| self.positions[at].1)
    }
}

/// 多重集合の一致まで確認した位置と、復元後の観測文字集合。
struct Repair {
    /// SDKの元の文字位置と、復元するUnicode文字。
    positions: Vec<(usize, char)>,
    /// 推測せずに復元した後の文字数。
    observed: BTreeMap<char, usize>,
}

/// 一つのSDK字形セルに連続して所属する文字。
struct Group {
    /// SDKの同一オブジェクト・矩形・原点。
    cell: Cell,
    /// セルの最初のSDK文字位置。
    index: usize,
    /// 一つだけ文字があるときの生のUnicode。
    scalar: char,
    /// UTF-16対は一文字として数えた、セルの文字数。
    count: usize,
}

/// 一文字に対応するセルだけを候補に残し、展開列は置換しない。
fn finish(group: Group, targets: &BTreeMap<char, Target>, positions: &mut Vec<(usize, char)>) {
    if group.count == 1 && targets.contains_key(&group.scalar) {
        positions.push((group.index, group.scalar));
    }
}

/// Windowsの字形名による16ビット切り詰めを、原文とセル個数から検証する。
pub(crate) fn read(
    pdfium: &Pdfium,
    text: *mut c_void,
    facts: &PageFacts,
    source: Option<&Facts>,
    observed: &mut BTreeMap<char, usize>,
) -> Corrections {
    let Some(source) = source else {
        return Corrections::default();
    };
    if !cfg!(windows)
        || source.named_supplementary.is_empty()
        || source.named_supplementary_unavailable
        || source.unavailable
        || source.glyph_ownership_unavailable
        || facts.source_unicode_accounting_unavailable
        || facts.errors != 0
        || facts.source_text_characters_missing != 0
        || facts.source_text_accounting_unavailable
        || facts.sdk_text_objects_without_characters != 0
        || !facts.actual_regions.is_empty()
        || *observed == source.scalars
    {
        return Corrections::default();
    }
    let Some(targets) = targets(&source.scalars, &source.named_supplementary) else {
        return Corrections::default();
    };
    let b = pdfium.bindings();
    let mut group: Option<Group> = None;
    let mut cells = Inventory::default();
    let mut positions = Vec::new();
    for index in 0..facts.characters {
        if facts.projected(index) {
            return Corrections::default();
        }
        if facts.is_paired_low(index) {
            continue;
        }
        match b.FPDFText_IsGenerated(text.cast(), index as i32) {
            1 => {
                if let Some(old) = group.take() {
                    finish(old, &targets, &mut positions);
                }
                continue;
            }
            0 => {}
            _ => return Corrections::default(),
        }
        let Some(cell) = Cell::read(pdfium, text, index as i32) else {
            return Corrections::default();
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
            return Corrections::default();
        };
        if group.as_ref().is_none_or(|g| g.cell != cell) {
            if let Some(old) = group.take() {
                finish(old, &targets, &mut positions);
            }
            if cells.insert(cell).is_none() {
                return Corrections::default();
            }
            group = Some(Group {
                cell,
                index,
                scalar,
                count: 0,
            });
        }
        group.as_mut().unwrap().count += 1;
    }
    if let Some(old) = group {
        finish(old, &targets, &mut positions);
    }
    let Some(repair) = prove(
        &source.scalars,
        observed,
        &targets,
        &positions,
        cells.len(),
        source.glyphs,
    ) else {
        return Corrections::default();
    };
    let mut fingerprint = unicode_facts::FINGERPRINT_START;
    for (index, _) in &repair.positions {
        fingerprint = unicode_facts::raw_geometry(pdfium, text, *index as i32, fingerprint);
    }
    *observed = repair.observed;
    Corrections {
        positions: repair.positions,
        fingerprint,
    }
}

/// 同じBMP値に対して原文で一意な補助文字と使用回数。
struct Target {
    /// 原文の正式な字形名が宣言するUnicode文字。
    scalar: char,
    /// この字形名の使用回数。
    count: usize,
}

/// 読み順と原文の通常文字に衝突しない候補だけを求める。
fn targets(
    declared: &BTreeMap<char, usize>,
    named: &BTreeMap<char, usize>,
) -> Option<BTreeMap<char, Target>> {
    if named.is_empty() || named.len() > 1024 {
        return None;
    }
    let mut remaining = declared.clone();
    for (&scalar, &count) in named {
        subtract(&mut remaining, scalar, count)?;
    }
    let mut output = BTreeMap::new();
    for (&scalar, &count) in named {
        if scalar as u32 <= 0xffff || count == 0 {
            return None;
        }
        let narrowed = char::from_u32(scalar as u32 & 0xffff)?;
        // SDK内で既に決めた双方向の順序を、文字置換だけで変えない。
        if unicode_bidi::bidi_class(scalar) != unicode_bidi::BidiClass::L
            || unicode_bidi::bidi_class(narrowed) != unicode_bidi::BidiClass::L
        {
            return None;
        }
        let normalized = bag_for(narrowed);
        if normalized.is_empty()
            || normalized
                .iter()
                .all(|(c, n)| remaining.get(c).copied().unwrap_or(0) >= *n)
            || output.insert(narrowed, Target { scalar, count }).is_some()
        {
            return None;
        }
    }
    Some(output)
}

/// 既存の照合と同じ正準分解を行い、数学文字の互換分解はしない。
fn bag_for(scalar: char) -> BTreeMap<char, usize> {
    let mut bag = BTreeMap::new();
    crate::source_text::unicode::add_scalar(&mut bag, scalar);
    bag
}

/// 多重集合から同じ正準文字を厳密に差し引き、過剰や整数桁あふれを拒否する。
fn subtract(bag: &mut BTreeMap<char, usize>, scalar: char, count: usize) -> Option<()> {
    for (c, n) in bag_for(scalar) {
        let n = n.checked_mul(count)?;
        let old = bag.get_mut(&c)?;
        *old = old.checked_sub(n)?;
        if *old == 0 {
            bag.remove(&c);
        }
    }
    Some(())
}

/// 原文・SDKのセル個数と、補正後の文字集合全体の一致を証明する。
fn prove(
    declared: &BTreeMap<char, usize>,
    observed: &BTreeMap<char, usize>,
    targets: &BTreeMap<char, Target>,
    positions: &[(usize, char)],
    cells: usize,
    glyphs: usize,
) -> Option<Repair> {
    if cells != glyphs || positions.is_empty() {
        return None;
    }
    let mut corrected = observed.clone();
    let mut counts = BTreeMap::<char, usize>::new();
    let mut output = Vec::with_capacity(positions.len());
    for &(index, scalar) in positions {
        let target = targets.get(&scalar)?;
        let count = counts.entry(scalar).or_default();
        *count += 1;
        if *count > target.count || output.last().is_some_and(|(old, _)| *old >= index) {
            return None;
        }
        subtract(&mut corrected, scalar, 1)?;
        for (c, n) in bag_for(target.scalar) {
            *corrected.entry(c).or_default() += n;
        }
        output.push((index, target.scalar));
    }
    (corrected == *declared).then_some(Repair {
        positions: output,
        observed: corrected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 本文と同じ正準分解で、文字数を独立に保持する。
    fn bag(text: &str) -> BTreeMap<char, usize> {
        let mut output = BTreeMap::new();
        for c in text.chars() {
            crate::source_text::unicode::add_scalar(&mut output, c);
        }
        output
    }

    /// 宣言した一文字と、SDKが返した16ビット値だけを対応付ける。
    #[test]
    fn declared_supplementary_scalar_replaces_only_its_proven_cell() {
        let declared = bag("A𐐌B");
        let observed = bag("AЌB");
        let targets = targets(&declared, &BTreeMap::from([('𐐌', 1)])).unwrap();
        assert_eq!(
            prove(&declared, &observed, &targets, &[(1, 'Ќ')], 3, 3).map(|r| r.positions),
            Some(vec![(1, '𐐌')])
        );
        assert!(prove(&declared, &observed, &targets, &[(1, 'Ќ')], 2, 3).is_none());
        assert!(prove(&declared, &bag("ЌB"), &targets, &[(1, 'Ќ')], 3, 3).is_none());
        assert!(prove(&declared, &observed, &targets, &[(1, 'Ќ'), (2, 'Ќ')], 3, 3).is_none());
    }

    /// 正準分解するBMP値からも、原文どおりの数学文字を復元する。
    #[test]
    fn canonical_decomposition_is_accounted_without_general_compatibility_folding() {
        for (original, narrowed) in [('\u{1d400}', '\u{d400}'), ('\u{1d15e}', '\u{d15e}')] {
            let declared = bag(&original.to_string());
            let observed = bag(&narrowed.to_string());
            let targets = targets(&declared, &BTreeMap::from([(original, 1)])).unwrap();
            assert_eq!(
                prove(&declared, &observed, &targets, &[(8, narrowed)], 1, 1).map(|r| r.positions),
                Some(vec![(8, original)])
            );
            assert!(prove(&bag("A"), &observed, &targets, &[(8, narrowed)], 1, 1).is_none());
        }
    }

    /// 文字が消えた場合・通常文字との衝突・同じ下位16ビットは推測しない。
    #[test]
    fn ambiguous_or_direction_changing_source_values_remain_unresolved() {
        let named = BTreeMap::from([('𐐌', 1)]);
        assert!(targets(&bag("𐐌Ќ"), &named).is_none());
        assert!(targets(&bag("𐐌К\u{301}"), &named).is_none());
        let other = '\u{2040c}';
        assert!(targets(
            &bag(&format!("𐐌{other}")),
            &BTreeMap::from([('𐐌', 1), (other, 1)])
        )
        .is_none());
        for original in ['😀', '\u{1e900}', '\u{1d167}'] {
            assert!(targets(
                &bag(&original.to_string()),
                &BTreeMap::from([(original, 1)])
            )
            .is_none());
        }
        let declared = bag("𐐌");
        let targets = targets(&declared, &named).unwrap();
        assert!(prove(&declared, &BTreeMap::new(), &targets, &[], 0, 1).is_none());
    }

    /// 分割された同一字形の再出現で、消えた空白のセルを埋め合わせない。
    #[test]
    fn split_unicode_pieces_cannot_mask_an_omitted_source_glyph() {
        let declared = bag("𐐌אA ");
        let observed = bag("אЌA");
        let targets = targets(&declared, &BTreeMap::from([('𐐌', 1)])).unwrap();
        let split = Cell::from_values(1, [0.; 6]).unwrap();
        let named = Cell::from_values(2, [1.; 6]).unwrap();
        let mut cells = Inventory::default();
        for cell in [split, named, split] {
            cells.insert(cell).unwrap();
        }
        assert_eq!(cells.len(), 2);
        assert!(prove(&declared, &observed, &targets, &[(1, 'Ќ')], cells.len(), 3).is_none());
    }

    proptest! {
        /// 繰り返し文字の個数を保存し、正常なセルとUTF-16復元済みの文字を変えない。
        #[test]
        fn recovery_preserves_multiplicity_and_existing_supplementary_values(count in 1usize..65, already_correct in 0usize..8) {
            let total = count + already_correct;
            let declared = bag(&format!("A{}B", "𐐌".repeat(total)));
            let observed = bag(&format!("A{}{}B", "Ќ".repeat(count), "𐐌".repeat(already_correct)));
            let targets = targets(&declared, &BTreeMap::from([('𐐌', total)])).unwrap();
            let positions: Vec<_> = (1..=count).map(|i| (i, 'Ќ')).collect();
            let recovered = prove(&declared, &observed, &targets, &positions, total + 2, total + 2).unwrap().positions;
            prop_assert_eq!(recovered, (1..=count).map(|i| (i, '𐐌')).collect::<Vec<_>>());
        }
    }
}
