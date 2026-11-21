//! Unicodeの既定Lを証明として使わず、固定した明示的なL区間だけを照合する。
#[path = "strong_ltr_data.rs"]
mod data;

/// Unicode 18の明示的な強いLTR文字にだけ、順序を保つ文字置換を許可する。
pub(crate) fn contains(scalar: char) -> bool {
    let code = scalar as u32;
    let at = data::RANGES.partition_point(|&(_, end)| end < code);
    data::RANGES
        .get(at)
        .is_some_and(|&(start, _)| start <= code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未割当・新規結合文字・RTL・中立記号を、既定Lとして受け入れない。
    #[test]
    fn only_explicit_strong_ltr_scalars_are_eligible() {
        for c in ['A', 'Ќ', '𐐌', '𝐀', '\u{1d15e}', '\u{d15e}'] {
            assert!(contains(c), "U+{:X}", c as u32);
        }
        for c in [
            '\u{0378}',
            '\u{11b60}',
            '\u{11b62}',
            '\u{1ccfa}',
            '\u{1ceba}',
            'א',
            '😀',
            '\u{1e900}',
            '\u{1d167}',
        ] {
            assert!(!contains(c), "U+{:X}", c as u32);
        }
    }

    /// 区間の両端と境界外を全区間で確認し、検索の飛び越しを防ぐ。
    #[test]
    fn generated_ranges_are_ordered_disjoint_scalar_intervals() {
        let mut previous = None;
        for &(start, end) in data::RANGES {
            assert!(start <= end && end <= 0x10ffff);
            assert!(end < 0xd800 || start > 0xdfff);
            assert!(previous.is_none_or(|old| old + 1 < start));
            assert!(contains(char::from_u32(start).unwrap()));
            assert!(contains(char::from_u32(end).unwrap()));
            if start > 0 {
                if let Some(c) = char::from_u32(start - 1) {
                    assert!(!contains(c));
                }
            }
            if end < 0x10ffff {
                if let Some(c) = char::from_u32(end + 1) {
                    assert!(!contains(c));
                }
            }
            previous = Some(end);
        }
    }
}
