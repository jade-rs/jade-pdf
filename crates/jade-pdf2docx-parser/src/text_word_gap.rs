//! encoded glyphの位置調整を変えず、語間として大きな正方向gapだけを文字列へ補う。

/// 既存空白・制御文字・CJKの字間を語間へ取り違えず、一つの区切りを必要とするか返す。
pub(crate) fn needs_separator(left: &str, right: &str, gap: f32, size: f32) -> bool {
    if !gap.is_finite() || !size.is_finite() || size <= 0.0 || gap < size * 0.2 {
        return false;
    }
    let (Some(left), Some(right)) = (left.chars().last(), right.chars().next()) else {
        return false;
    };
    word_spaced_character(left) && word_spaced_character(right)
}

/// 語間を持つ文字・句読点を対象にし、CJK・全角のtrackingは従来の字間として残す。
fn word_spaced_character(scalar: char) -> bool {
    let code = u32::from(scalar);
    if matches!(code, 0x2e80..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff | 0xff00..=0xffef | 0x20000..=0x323af)
    {
        return false;
    }
    scalar.is_alphanumeric() || scalar.is_ascii_punctuation() || (0x2010..=0x2027).contains(&code)
}
