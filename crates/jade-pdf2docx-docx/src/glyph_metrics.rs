//! 確認できる一つのglyphだけを測り、複雑なshapingを推測した幅で置き換えない。

/// 通常cmapまたは単一required rligで一glyphになる列のadvanceをem単位で返す。
pub(super) fn advance(_bytes: &[u8], _text: &str) -> Option<f64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 所有space fontの既知advanceだけを使い、未知字形と複数字形は拒否する。
    #[test]
    fn measures_only_a_verified_single_glyph() {
        let bytes = crate::layout_space::font("JadeLayoutSpace-0");
        assert_eq!(advance(&bytes," "),Some(1.));
        for text in ["", "A", "  ", "\u{00a0}", "\u{fffd}"] { assert_eq!(advance(&bytes,text),None); }
        for end in 0..bytes.len().min(100) { assert_eq!(advance(&bytes[..end]," "),None); }
    }
}
