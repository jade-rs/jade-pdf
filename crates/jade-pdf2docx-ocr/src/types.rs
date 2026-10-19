use std::path::PathBuf;

/// 呼び出し元が明示する本文方向。自動判定は行わない。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum OcrTextDirection {
    /// 横書きの行を上から下へ読む。
    #[default]
    Horizontal,
    /// 縦書きの列を右から左へ、各列内を上から下へ読む。
    Vertical,
}

/// PARSeq 認識失敗時のハンドリング方針。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseqFailurePolicy {
    /// 認識失敗を即時エラーとして返す。
    FailFast,
    /// 認識失敗時は信頼度0.0の診断ブロックを返す。本文には使用しない。
    FallbackToBandLabel,
}

/// OCR 実行リクエスト。
#[derive(Debug, Clone, PartialEq)]
pub struct OcrRequest {
    /// 画像内の本文方向。既定は横書き。
    pub text_direction: OcrTextDirection,
    /// 紙面の陰影補正を明示的に有効にする。既定は無効。
    pub paper_shading: bool,
    /// 本文の容量上限。未指定時は従来どおり容量を制限しない。
    pub text_limits: Option<crate::OcrTextLimits>,
    /// 呼び出し元が管理する入力ソース識別子。
    pub source_id: String,
    /// OCR 対象画像パス（将来の実行入力）。
    pub image_path: Option<PathBuf>,
    /// OCR対象の左上原点の画素範囲[x0,y0,x1,y1]。終端は含まず、画像外は拒否する。
    pub image_region: Option<[usize; 4]>,
    /// テスト用のモック行入力（feature PoC 向け）。
    pub mock_lines: Option<Vec<String>>,
    /// 行抽出で使う2値化しきい値（0-255）。
    pub binarize_threshold: Option<u8>,
    /// PARSeq モデルパス（line OCR 用）。
    pub parseq_model_path: Option<PathBuf>,
    /// PARSeq 文字セット定義 YAML パス。
    pub parseq_charset_path: Option<PathBuf>,
    /// PARSeq 認識失敗時のハンドリング方針。
    pub parseq_failure_policy: Option<ParseqFailurePolicy>,
    /// PARSeq 認識結果に要求する最小平均信頼度（0.0 - 1.0）。
    pub parseq_min_confidence: Option<f32>,
    /// 返却する OCR 行数の上限。
    pub max_output_lines: Option<usize>,
    /// 返却対象にする OCR 行の最小文字数。
    pub min_line_length: Option<usize>,
    /// 許容する記号率（0.0 - 1.0）。超過行は除外。
    pub max_symbol_ratio: Option<f32>,
}

impl OcrRequest {
    /// OCR リクエストを生成する。
    pub fn new(source_id: impl Into<String>) -> Self {
        Self {
            text_direction: OcrTextDirection::Horizontal,
            paper_shading: false,
            text_limits: None,
            source_id: source_id.into(),
            image_path: None,
            image_region: None,
            mock_lines: None,
            binarize_threshold: None,
            parseq_model_path: None,
            parseq_charset_path: None,
            parseq_failure_policy: None,
            parseq_min_confidence: None,
            max_output_lines: None,
            min_line_length: None,
            max_symbol_ratio: None,
        }
    }

    /// OCR 対象画像パスを設定する。
    pub fn with_image_path(mut self, image_path: impl Into<PathBuf>) -> Self {
        self.image_path = Some(image_path.into());
        self
    }

    /// 本文方向を明示する。縦書きでも座標は元画像のまま返す。
    pub fn with_text_direction(mut self, direction: OcrTextDirection) -> Self {
        self.text_direction = direction;
        self
    }

    /// 紙の陰影補正を明示的に切り替える。
    pub fn with_paper_shading(mut self, enabled: bool) -> Self {
        self.paper_shading = enabled;
        self
    }

    /// ページ画像の指定領域だけを認識する。白紙確認もこの領域へ限定する。
    pub fn with_image_region(mut self, region: [usize; 4]) -> Self {
        self.image_region = Some(region);
        self
    }

    /// 本文を切り捨てず、上限超過を資源エラーとして返す。
    pub fn with_text_limits(mut self, limits: crate::OcrTextLimits) -> Self {
        self.text_limits = Some(limits);
        self
    }

    /// テスト用モック行を設定する。
    pub fn with_mock_lines(
        mut self,
        mock_lines: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.mock_lines = Some(mock_lines.into_iter().map(Into::into).collect());
        self
    }

    /// 行抽出で使う2値化しきい値を設定する。
    pub fn with_binarize_threshold(mut self, threshold: u8) -> Self {
        self.binarize_threshold = Some(threshold);
        self
    }

    /// PARSeq のモデルと文字セットパスを設定する。
    pub fn with_parseq_paths(
        mut self,
        model_path: impl Into<PathBuf>,
        charset_path: impl Into<PathBuf>,
    ) -> Self {
        self.parseq_model_path = Some(model_path.into());
        self.parseq_charset_path = Some(charset_path.into());
        self
    }

    /// PARSeq 認識失敗時のハンドリング方針を設定する。
    pub fn with_parseq_failure_policy(mut self, policy: ParseqFailurePolicy) -> Self {
        self.parseq_failure_policy = Some(policy);
        self
    }

    /// PARSeq 認識結果に要求する最小平均信頼度を設定する。
    pub fn with_parseq_min_confidence(mut self, min_confidence: f32) -> Self {
        self.parseq_min_confidence = Some(min_confidence);
        self
    }

    /// 返却する OCR 行数の上限を設定する。
    pub fn with_max_output_lines(mut self, max_output_lines: usize) -> Self {
        self.max_output_lines = Some(max_output_lines);
        self
    }

    /// 返却対象にする OCR 行の最小文字数を設定する。
    pub fn with_min_line_length(mut self, min_line_length: usize) -> Self {
        self.min_line_length = Some(min_line_length);
        self
    }

    /// 許容する記号率上限を設定する。
    pub fn with_max_symbol_ratio(mut self, max_symbol_ratio: f32) -> Self {
        self.max_symbol_ratio = Some(max_symbol_ratio);
        self
    }

    /// リクエストが実行可能な状態か検証する。
    pub fn validate(&self) -> Result<(), OcrError> {
        if let Some(limits) = self.text_limits {
            crate::OcrTextBudget::new(limits).check_line_bytes(1)?;
        }
        self.validate_fields()
    }

    /// 白紙検証の前にも設定を検査するが、本文容量は実際の認識行で検査する。
    pub(crate) fn validate_fields(&self) -> Result<(), OcrError> {
        if self.source_id.trim().is_empty() {
            return Err(OcrError::InvalidRequest(
                "source_id must not be empty".to_string(),
            ));
        }
        if let Some(path) = self.image_path.as_ref()
            && path.as_os_str().is_empty()
        {
            return Err(OcrError::InvalidRequest(
                "image_path must not be empty".to_string(),
            ));
        }
        if let Some(lines) = self.mock_lines.as_ref() {
            if lines.is_empty() {
                return Err(OcrError::InvalidRequest(
                    "mock_lines must not be empty".to_string(),
                ));
            }
            if lines.iter().all(|line| line.trim().is_empty()) {
                return Err(OcrError::InvalidRequest(
                    "mock_lines must contain at least one non-empty line".to_string(),
                ));
            }
        }
        if let Some([x0, y0, x1, y1]) = self.image_region
            && (x0 >= x1 || y0 >= y1 || self.image_path.is_none() || self.mock_lines.is_some())
        {
            return Err(OcrError::InvalidRequest(
                "image region requires a non-empty rectangle and an image without mock lines"
                    .into(),
            ));
        }
        let model_set = self.parseq_model_path.is_some();
        let charset_set = self.parseq_charset_path.is_some();
        if model_set != charset_set {
            return Err(OcrError::InvalidRequest(
                "parseq_model_path and parseq_charset_path must be set together".to_string(),
            ));
        }
        if let Some(min_confidence) = self.parseq_min_confidence
            && (!min_confidence.is_finite() || !(0.0..=1.0).contains(&min_confidence))
        {
            return Err(OcrError::InvalidRequest(
                "parseq_min_confidence must be a finite value in [0.0, 1.0]".to_string(),
            ));
        }
        if let Some(max_output_lines) = self.max_output_lines
            && max_output_lines == 0
        {
            return Err(OcrError::InvalidRequest(
                "max_output_lines must be greater than 0".to_string(),
            ));
        }
        if let Some(max_symbol_ratio) = self.max_symbol_ratio
            && (!max_symbol_ratio.is_finite() || !(0.0..=1.0).contains(&max_symbol_ratio))
        {
            return Err(OcrError::InvalidRequest(
                "max_symbol_ratio must be a finite value in [0.0, 1.0]".to_string(),
            ));
        }
        Ok(())
    }
}

/// OCR 実行時の失敗種別。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrError {
    /// 本文の容量または行数上限を超過。診断fallbackでは回復しない。
    ResourceLimit(String),
    /// 呼び出し条件が不正。
    InvalidRequest(String),
    /// OCR エンジンが失敗。
    EngineFailure(String),
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrError::ResourceLimit(message) => write!(f, "OCR resource limit: {message}"),
            OcrError::InvalidRequest(message) => write!(f, "invalid OCR request: {message}"),
            OcrError::EngineFailure(message) => write!(f, "OCR engine failure: {message}"),
        }
    }
}

impl std::error::Error for OcrError {}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn validate_rejects_empty_source_id() {
        let request = OcrRequest::new("   ");
        assert!(matches!(
            request.validate(),
            Err(OcrError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_rejects_empty_mock_lines() {
        let request = OcrRequest::new("fixture:sample").with_mock_lines(Vec::<String>::new());
        assert!(
            matches!(request.validate(), Err(OcrError::InvalidRequest(message)) if message.contains("mock_lines"))
        );
    }

    #[test]
    fn validate_rejects_whitespace_only_mock_lines() {
        let request = OcrRequest::new("fixture:sample").with_mock_lines(["  ", "\t"]);
        assert!(
            matches!(request.validate(), Err(OcrError::InvalidRequest(message)) if message.contains("non-empty"))
        );
    }

    #[test]
    fn validate_accepts_non_empty_mock_lines() {
        let request = OcrRequest::new("fixture:sample").with_mock_lines(["line1"]);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn validate_rejects_parseq_model_without_charset() {
        let mut request = OcrRequest::new("fixture:sample");
        request.parseq_model_path = Some(PathBuf::from("model.onnx"));
        assert!(
            matches!(request.validate(), Err(OcrError::InvalidRequest(message)) if message.contains("set together"))
        );
    }

    #[test]
    fn validate_accepts_parseq_paths_when_both_set() {
        let request =
            OcrRequest::new("fixture:sample").with_parseq_paths("model.onnx", "charset.yaml");
        assert!(request.validate().is_ok());
    }

    #[test]
    fn validate_accepts_parseq_failure_policy_without_parseq_paths() {
        let request = OcrRequest::new("fixture:sample")
            .with_parseq_failure_policy(ParseqFailurePolicy::FallbackToBandLabel);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn validate_rejects_parseq_min_confidence_out_of_range() {
        let request = OcrRequest::new("fixture:sample").with_parseq_min_confidence(1.2);
        assert!(
            matches!(request.validate(), Err(OcrError::InvalidRequest(message)) if message.contains("parseq_min_confidence"))
        );
    }

    #[test]
    fn validate_accepts_parseq_min_confidence_in_range() {
        let request = OcrRequest::new("fixture:sample").with_parseq_min_confidence(0.6);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_max_output_lines() {
        let request = OcrRequest::new("fixture:sample").with_max_output_lines(0);
        assert!(
            matches!(request.validate(), Err(OcrError::InvalidRequest(message)) if message.contains("max_output_lines"))
        );
    }

    #[test]
    fn validate_rejects_max_symbol_ratio_out_of_range() {
        let request = OcrRequest::new("fixture:sample").with_max_symbol_ratio(1.1);
        assert!(
            matches!(request.validate(), Err(OcrError::InvalidRequest(message)) if message.contains("max_symbol_ratio"))
        );
    }

    #[test]
    fn validate_accepts_output_filters() {
        let request = OcrRequest::new("fixture:sample")
            .with_max_output_lines(3)
            .with_min_line_length(2)
            .with_max_symbol_ratio(0.8);
        assert!(request.validate().is_ok());
    }

    proptest! {
        #[test]
        fn validate_rejects_whitespace_only_source_id(value in "\\s*") {
            let request = OcrRequest::new(value);
            prop_assert!(request.validate().is_err());
        }

        #[test]
        fn validate_accepts_non_whitespace_source_id(value in ".+") {
            prop_assume!(!value.trim().is_empty());
            let request = OcrRequest::new(value);
            prop_assert!(request.validate().is_ok());
        }
    }
}
