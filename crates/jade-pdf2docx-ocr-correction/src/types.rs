/// 候補生成時の入力・出力・比較回数の上限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorrectionLimits {
    /// 一行のUnicode文字数の上限。
    pub max_text_chars: usize,
    /// 重複除去前の辞書語数の上限。
    pub max_dictionary_terms: usize,
    /// 辞書語一つのUnicode文字数の上限。
    pub max_term_chars: usize,
    /// 取り違え規則数の上限。
    pub max_confusions: usize,
    /// 一行に返す候補数の上限。超過時は部分結果を返さず拒否する。
    pub max_candidates: usize,
    /// 一候補で許容する置換文字数。
    pub max_substitutions: usize,
    /// 一行の辞書照合で行う文字比較回数の上限。
    pub max_comparisons: usize,
}

impl Default for CorrectionLimits {
    /// 小さな文書別辞書と行単位処理を想定した上限を返す。
    fn default() -> Self {
        Self {
            max_text_chars: 2048,
            max_dictionary_terms: 512,
            max_term_chars: 32,
            max_confusions: 128,
            max_candidates: 64,
            max_substitutions: 1,
            max_comparisons: 1_000_000,
        }
    }
}

/// 候補を生成する信頼度範囲と資源上限。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorrectionOptions {
    /// この値未満の行だけを候補生成の対象にする。校正された確率ではない。
    pub max_confidence: f32,
    /// 候補生成の資源上限。
    pub limits: CorrectionLimits,
}

impl Default for CorrectionOptions {
    /// 高信頼の認識を保留する初期設定を返す。閾値は評価データに合わせて調整する。
    fn default() -> Self {
        Self {
            max_confidence: 0.85,
            limits: CorrectionLimits::default(),
        }
    }
}

/// 候補生成・候補選択での検証失敗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectionError {
    /// 非有限値または0から1の範囲外のOCR信頼度。
    InvalidConfidence,
    /// 不正な辞書・規則・上限設定。
    InvalidConfiguration(&'static str),
    /// 入力・候補数・処理量の上限超過。
    LimitExceeded(&'static str),
    /// 不正番号・重複・範囲重複を含む候補選択。
    InvalidSelection(&'static str),
}

impl std::fmt::Display for CorrectionError {
    /// 呼び出し側へ検証失敗の種類と対象を伝える。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfidence => write!(formatter, "invalid OCR confidence"),
            Self::InvalidConfiguration(reason) => {
                write!(formatter, "invalid correction configuration: {reason}")
            }
            Self::LimitExceeded(reason) => write!(formatter, "correction limit exceeded: {reason}"),
            Self::InvalidSelection(reason) => {
                write!(formatter, "invalid correction selection: {reason}")
            }
        }
    }
}

impl std::error::Error for CorrectionError {}
