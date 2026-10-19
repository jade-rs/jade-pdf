use crate::{CorrectionCandidate, CorrectionError, CorrectionOptions, CorrectionReport};
use std::collections::{BTreeMap, BTreeSet};

/// 辞書語の原文と比較用文字列。
#[derive(Debug)]
struct Term {
    /// 出力候補に用いる辞書語。
    text: String,
    /// Unicode文字単位の比較用配列。
    chars: Vec<char>,
}

/// 明示辞書と有向の文字置換規則を使う、外部runtime依存のない補正器。
#[derive(Debug)]
pub struct Corrector {
    /// 同じ文字数の辞書語だけを比較する索引。
    terms: BTreeMap<usize, Vec<Term>>,
    /// 認識文字から辞書文字への、利用側が承認した置換規則。
    confusions: BTreeSet<(char, char)>,
    /// 信頼度と比較処理の上限。
    options: CorrectionOptions,
}

impl Corrector {
    /// 文書別辞書と有向規則を検証する。単文字語・空白語・数字等の置換規則は拒否する。
    pub fn new(
        dictionary: &[&str],
        confusions: &[(char, char)],
        options: CorrectionOptions,
    ) -> Result<Self, CorrectionError> {
        let limits = options.limits;
        if !valid_confidence(options.max_confidence)
            || [
                limits.max_text_chars,
                limits.max_dictionary_terms,
                limits.max_term_chars,
                limits.max_confusions,
                limits.max_candidates,
                limits.max_substitutions,
                limits.max_comparisons,
            ]
            .contains(&0)
            || limits.max_term_chars < 2
            || limits.max_substitutions > limits.max_term_chars
        {
            return Err(CorrectionError::InvalidConfiguration("options"));
        }
        if dictionary.len() > limits.max_dictionary_terms {
            return Err(CorrectionError::LimitExceeded("dictionary terms"));
        }
        if confusions.len() > limits.max_confusions {
            return Err(CorrectionError::LimitExceeded("confusion rules"));
        }
        for &(source, target) in confusions {
            if source == target || protected_character(source) || protected_character(target) {
                return Err(CorrectionError::InvalidConfiguration(
                    "protected confusion characters",
                ));
            }
        }
        let mut unique = BTreeSet::new();
        let mut terms: BTreeMap<usize, Vec<Term>> = BTreeMap::new();
        for &text in dictionary {
            let chars = text
                .chars()
                .take(limits.max_term_chars.saturating_add(1))
                .collect::<Vec<_>>();
            if chars.len() > limits.max_term_chars {
                return Err(CorrectionError::LimitExceeded("term characters"));
            }
            if chars.len() < 2 || chars.iter().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(CorrectionError::InvalidConfiguration("dictionary term"));
            }
            if unique.insert(text) {
                terms.entry(chars.len()).or_default().push(Term {
                    text: text.into(),
                    chars,
                });
            }
        }
        Ok(Self {
            terms,
            confusions: confusions.iter().copied().collect(),
            options,
        })
    }

    /// 低信頼行の候補を返す。上限超過時には、切り捨てた部分結果を返さない。
    pub fn analyze(
        &self,
        text: &str,
        confidence: f32,
    ) -> Result<CorrectionReport, CorrectionError> {
        if !valid_confidence(confidence) {
            return Err(CorrectionError::InvalidConfidence);
        }
        let chars = text
            .chars()
            .take(self.options.limits.max_text_chars.saturating_add(1))
            .collect::<Vec<_>>();
        if chars.len() > self.options.limits.max_text_chars {
            return Err(CorrectionError::LimitExceeded("text characters"));
        }
        let mut report = CorrectionReport {
            original: text.into(),
            confidence,
            candidates: Vec::new(),
        };
        if confidence >= self.options.max_confidence || chars.is_empty() {
            return Ok(report);
        }
        let mut remaining = self.options.limits.max_comparisons;
        let mut protected = vec![false; chars.len()];
        // 完全一致の辞書語に重なる候補は、語の長さにかかわらず除外する。
        for (&length, terms) in self.terms.range(..=chars.len()) {
            for (start, window) in chars.windows(length).enumerate() {
                for term in terms {
                    if self
                        .distance(window, term, false, &mut remaining)?
                        .is_some()
                    {
                        protected[start..start + length].fill(true);
                    }
                }
            }
        }
        let offsets = text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .collect::<Vec<_>>();
        for (&length, terms) in self.terms.range(..=chars.len()) {
            for (start, window) in chars.windows(length).enumerate() {
                if protected[start..start + length].iter().any(|&value| value) {
                    continue;
                }
                for term in terms {
                    if let Some(substitutions) =
                        self.distance(window, term, true, &mut remaining)?
                    {
                        if report.candidates.len() >= self.options.limits.max_candidates {
                            return Err(CorrectionError::LimitExceeded("candidates"));
                        }
                        let span = offsets[start]..offsets[start + length];
                        report.candidates.push(CorrectionCandidate {
                            original: text[span.clone()].into(),
                            span,
                            replacement: term.text.clone(),
                            substitutions,
                        });
                    }
                }
            }
        }
        report.candidates.sort_by(|left, right| {
            (
                left.span.start,
                left.span.end,
                left.substitutions,
                &left.replacement,
            )
                .cmp(&(
                    right.span.start,
                    right.span.end,
                    right.substitutions,
                    &right.replacement,
                ))
        });
        Ok(report)
    }

    /// 文字比較を予算へ加算し、完全一致または明示規則だけの置換数を返す。
    fn distance(
        &self,
        source: &[char],
        term: &Term,
        allow_confusions: bool,
        remaining: &mut usize,
    ) -> Result<Option<usize>, CorrectionError> {
        let mut substitutions = 0;
        for (&source, &target) in source.iter().zip(&term.chars) {
            *remaining = remaining
                .checked_sub(1)
                .ok_or(CorrectionError::LimitExceeded("character comparisons"))?;
            if source != target {
                if !allow_confusions || !self.confusions.contains(&(source, target)) {
                    return Ok(None);
                }
                substitutions += 1;
                if substitutions > self.options.limits.max_substitutions {
                    return Ok(None);
                }
            }
        }
        Ok(Some(substitutions))
    }
}

/// 非有限値と範囲外の信頼度を除外する。
fn valid_confidence(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

/// 数字・英数字識別子・漢数字・空白・記号を置換規則から保護する。
fn protected_character(value: char) -> bool {
    value.is_ascii()
        || !value.is_alphabetic()
        || value.is_numeric()
        || ('\u{ff01}'..='\u{ff5e}').contains(&value)
        || "〇零一二三四五六七八九十百千万億兆壱弐参拾萬".contains(value)
}
