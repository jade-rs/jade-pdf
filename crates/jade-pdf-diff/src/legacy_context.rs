//! Unicode 18の明示的な方向属性で、旧数式符号の置換文脈を検証する。
#[path = "legacy_context_data.rs"]
mod data;

/// 文字配置がRTL候補として扱う区間も避け、方向を変えない文脈だけを許す。
pub(crate) fn contains(c: char) -> bool {
    if crate::text::is_rtl(c) {
        return false;
    }
    let code = c as u32;
    let at = data::RANGES.partition_point(|&(_, end)| end < code);
    data::RANGES
        .get(at)
        .is_some_and(|&(start, _)| start <= code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 結合記号や引用符を既知のLTR文脈に含め、RTL・制御・私用・未割当を拒否する。
    #[test]
    fn context_is_explicitly_assigned_and_never_uses_missing_defaults() {
        for c in [
            'A',
            'ℓ',
            '≤',
            '“',
            '”',
            'é',
            '\u{0301}',
            '\u{11b60}',
            '∑',
            '3',
        ] {
            assert!(contains(c), "U+{:X}", c as u32);
        }
        for c in [
            'א',
            'ب',
            '\u{064b}',
            '\u{1e900}',
            '\u{0661}',
            '\u{202e}',
            '\u{2066}',
            '\u{2067}',
            '\u{2069}',
            '\u{0378}',
            '\u{e000}',
            '\u{f04b}',
            '\u{f0000}',
        ] {
            assert!(!contains(c), "U+{:X}", c as u32);
        }
    }

    /// 区間を独立に線形照合し、境界・連続区間・私用領域の混入を検出する。
    #[test]
    fn generated_ranges_are_disjoint_assigned_scalar_intervals() {
        let mut previous = None;
        for &(start, end) in data::RANGES {
            assert!(start <= end && end <= 0x10ffff);
            assert!(end < 0xd800 || start > 0xdfff);
            for (low, high) in [(0xe000, 0xf8ff), (0xf0000, 0xffffd), (0x100000, 0x10fffd)] {
                assert!(end < low || start > high);
            }
            assert!(previous.is_none_or(|old| old + 1 < start));
            for code in [start, end] {
                let c = char::from_u32(code).unwrap();
                assert_eq!(contains(c), !crate::text::is_rtl(c));
                assert!(!matches!(code, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd));
            }
            previous = Some(end);
        }
    }
}
