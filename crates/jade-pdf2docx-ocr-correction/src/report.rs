use crate::CorrectionError;
use std::ops::Range;

/// 辞書と明示規則に一致した補正候補。正解を保証するものではない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionCandidate {
    /// 原文内のUTF-8 byte範囲。終端は含まない。
    pub span: Range<usize>,
    /// 範囲内の原文。
    pub original: String,
    /// 辞書内の置換候補。
    pub replacement: String,
    /// 規則に一致した置換文字数。少ない候補を同一範囲内で先に返す。
    pub substitutions: usize,
}

/// 原文と候補を分離して保持する行単位の結果。
#[derive(Debug, Clone, PartialEq)]
pub struct CorrectionReport {
    /// 解析時の原文。候補適用後も変更しない。
    pub(crate) original: String,
    /// 解析時のOCR信頼度。
    pub(crate) confidence: f32,
    /// 原文位置・置換数・辞書語順で安定した補正候補。
    pub(crate) candidates: Vec<CorrectionCandidate>,
}

impl CorrectionReport {
    /// 空白・反復文字・数字を含む解析時の原文を返す。
    pub fn original(&self) -> &str {
        &self.original
    }

    /// OCRエンジンから受け取った信頼度をそのまま返す。
    pub fn confidence(&self) -> f32 {
        self.confidence
    }

    /// 確認・選択用の候補を返す。曖昧な候補も勝手に一つへ絞らない。
    pub fn candidates(&self) -> &[CorrectionCandidate] {
        &self.candidates
    }

    /// 明示した候補番号だけを適用した別の文字列を返す。原文と結果自体は変更しない。
    /// 重複・重なる範囲・存在しない番号は、部分的に適用せず拒否する。
    pub fn apply_selected(&self, indices: &[usize]) -> Result<String, CorrectionError> {
        if indices.len() > self.candidates.len() {
            return Err(CorrectionError::InvalidSelection("too many selections"));
        }
        let mut selected = indices
            .iter()
            .map(|&index| {
                self.candidates
                    .get(index)
                    .ok_or(CorrectionError::InvalidSelection("candidate index"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        selected.sort_by_key(|candidate| candidate.span.start);
        let mut result = String::with_capacity(self.original.len());
        let mut end = 0;
        for candidate in selected {
            if candidate.span.start < end {
                return Err(CorrectionError::InvalidSelection("overlapping candidates"));
            }
            result.push_str(&self.original[end..candidate.span.start]);
            result.push_str(&candidate.replacement);
            end = candidate.span.end;
        }
        result.push_str(&self.original[end..]);
        Ok(result)
    }
}
