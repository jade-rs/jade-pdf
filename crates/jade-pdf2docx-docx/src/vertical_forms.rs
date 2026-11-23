//! 縦書き用の約物（Unicode の縦書き presentation form）を通常の全角約物へ戻す。
//!
//! DOCX は横組みで組み直すので、縦書き用の字形の符号（「︵」U+FE35 など）のままだと
//! 横組みで横倒しの括弧に見える。lawrenceanum の kanpo-parser と同じ対応で戻す。

/// 縦書き presentation form に対応する通常の全角約物。対象外の字はそのまま返す。
pub(crate) fn horizontal_form(ch: char) -> char {
    match ch {
        '\u{FE10}' => '，',
        '\u{FE11}' => '、',
        '\u{FE12}' => '。',
        '\u{FE13}' => '：',
        '\u{FE14}' => '；',
        '\u{FE15}' => '！',
        '\u{FE16}' => '？',
        '\u{FE17}' => '〖',
        '\u{FE18}' => '〗',
        '\u{FE19}' => '…',
        '\u{FE30}' => '‥',
        '\u{FE31}' => '—',
        '\u{FE32}' => '–',
        '\u{FE33}' | '\u{FE34}' => '｜',
        '\u{FE35}' => '（',
        '\u{FE36}' => '）',
        '\u{FE37}' => '｛',
        '\u{FE38}' => '｝',
        '\u{FE39}' | '\u{FE47}' => '〔',
        '\u{FE3A}' | '\u{FE48}' => '〕',
        '\u{FE3B}' => '【',
        '\u{FE3C}' => '】',
        '\u{FE3D}' => '《',
        '\u{FE3E}' => '》',
        '\u{FE3F}' => '〈',
        '\u{FE40}' => '〉',
        '\u{FE41}' => '「',
        '\u{FE42}' => '」',
        '\u{FE43}' => '『',
        '\u{FE44}' => '』',
        other => other,
    }
}

/// 文字列中の縦書き presentation form を通常の全角約物へ戻す。
pub(crate) fn normalize(text: &str) -> Option<String> {
    text.chars()
        .any(|ch| horizontal_form(ch) != ch)
        .then(|| text.chars().map(horizontal_form).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 縦書きの括弧・句読点を横組みの字へ戻し、ほかの字は変えない。
    #[test]
    fn maps_vertical_presentation_forms_to_full_width_punctuation() {
        assert_eq!(
            normalize("︵注︶﹁可﹂︑︒").as_deref(),
            Some("（注）「可」、。")
        );
        assert_eq!(normalize("（注）"), None);
    }
}
