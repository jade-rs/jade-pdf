use crate::OcrError;

/// OCR ブロックの由来。診断用ラベルを認識本文と区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrTextKind {
    /// 推論器が認識した本文。
    Recognized,
    /// 呼び出し元が明示的に渡した検証用本文。
    Mock,
    /// 認識に失敗した行の座標。本文として使用しない。
    BandDiagnostic,
}

/// OCR 1ブロック分の抽出結果。
#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextBlock {
    /// 切り出し前の入力画像上の画素範囲[x0,y0,x1,y1]。mock・診断では未指定。
    pub bounds: Option<[usize; 4]>,
    /// 認識本文または診断文字列。用途は `kind` で判別する。
    pub text: String,
    /// 推論器の認識信頼度（0.0 - 1.0）。診断は0.0、明示mockは1.0。
    pub confidence: f32,
    /// 認識本文・明示mock・診断の区別。
    pub kind: OcrTextKind,
}

/// OCR 実行結果。診断用フォールバックを含む場合もある。
#[derive(Debug, Clone, PartialEq)]
pub struct OcrResult {
    /// 実行対象ソース識別子。
    pub source_id: String,
    /// 抽出したテキスト・診断ブロック列。
    pub blocks: Vec<OcrTextBlock>,
}

impl OcrResult {
    /// 本文検証を共通化し、実画像の行座標を文書変換まで保持する。
    /// 座標あり・なしの混在は曖昧な配置を避けるため拒否する。
    pub fn into_page_content(self) -> Result<crate::OcrPageContent, OcrError> {
        let positioned = self.blocks.iter().any(|block| block.bounds.is_some());
        if !positioned {
            return self.into_text_lines().map(crate::OcrPageContent::Text);
        }
        let bounds = self
            .blocks
            .iter()
            .map(|block| match block.bounds {
                Some([x0, y0, x1, y1]) if x0 < x1 && y0 < y1 => Ok([x0, y0, x1, y1]),
                _ => Err(OcrError::EngineFailure(
                    "invalid or mixed OCR line bounds".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let lines = self.into_text_lines()?;
        Ok(crate::OcrPageContent::PositionedText(
            lines
                .into_iter()
                .zip(bounds)
                .map(|(text, bounds)| crate::PositionedOcrLine { text, bounds })
                .collect(),
        ))
    }

    /// 本文として使用できる行を返す。診断・不正な信頼度・空行があれば全体を拒否する。
    ///
    /// 明示mockは検証用途として許容する。実OCRの精度検証には `kind` も確認する。
    /// 認識本文は前後の空白も保持し、明示mockだけ従来どおり前後を整理する。
    pub fn into_text_lines(self) -> Result<Vec<String>, OcrError> {
        if self.blocks.is_empty() {
            return Err(OcrError::EngineFailure("no OCR text blocks".into()));
        }
        self.blocks
            .into_iter()
            .map(|block| {
                if block.kind == OcrTextKind::BandDiagnostic {
                    return Err(OcrError::EngineFailure(
                        "OCR band diagnostic cannot be used as document text".into(),
                    ));
                }
                if !block.confidence.is_finite() || !(0.0..=1.0).contains(&block.confidence) {
                    return Err(OcrError::EngineFailure("invalid OCR confidence".into()));
                }
                if block.text.trim().is_empty() {
                    return Err(OcrError::EngineFailure("empty OCR text block".into()));
                }
                let text = match block.kind {
                    OcrTextKind::Mock => block.text.trim().to_string(),
                    _ => block.text,
                };
                Ok(text)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{OcrResult, OcrTextBlock, OcrTextKind};

    /// 座標付き本文も前後の空白を保持し、不正座標・混在・診断を拒否する。
    #[test]
    fn page_content_validates_positioned_blocks() {
        let block = OcrTextBlock {
            text: "  body  ".into(),
            confidence: 0.9,
            kind: OcrTextKind::Recognized,
            bounds: Some([10, 20, 30, 40]),
        };
        let result = OcrResult {
            source_id: "positioned".into(),
            blocks: vec![block.clone()],
        };
        assert_eq!(
            result.into_page_content().unwrap(),
            crate::OcrPageContent::PositionedText(vec![crate::PositionedOcrLine {
                text: "  body  ".into(),
                bounds: [10, 20, 30, 40]
            }])
        );
        for invalid in [
            OcrTextBlock {
                bounds: None,
                ..block.clone()
            },
            OcrTextBlock {
                bounds: Some([10, 20, 10, 40]),
                ..block.clone()
            },
            OcrTextBlock {
                bounds: Some([10, 20, 30, 10]),
                ..block.clone()
            },
            OcrTextBlock {
                confidence: f32::NAN,
                ..block.clone()
            },
            OcrTextBlock {
                kind: OcrTextKind::BandDiagnostic,
                ..block.clone()
            },
        ] {
            let result = OcrResult {
                source_id: "invalid".into(),
                blocks: vec![block.clone(), invalid],
            };
            assert!(result.into_page_content().is_err());
        }
    }

    /// 認識本文を文書用の行へ取り出す際にも、反復文字・括弧・空白を保持する。
    #[test]
    fn document_lines_preserve_recognized_text() {
        let text = " \t111111 123123123 乙）\u{3000}";
        let result = OcrResult {
            source_id: "fixture:unprocessed".into(),
            blocks: vec![OcrTextBlock {
                bounds: None,
                text: text.into(),
                confidence: 0.9,
                kind: OcrTextKind::Recognized,
            }],
        };
        assert_eq!(result.into_text_lines().unwrap(), [text]);
    }

    /// 診断を含む結果は一部だけでも本文として公開しない。
    #[test]
    fn document_lines_reject_mixed_diagnostic_result() {
        let result = OcrResult {
            source_id: "fixture:diagnostic".into(),
            blocks: vec![
                OcrTextBlock {
                    bounds: None,
                    text: "recognized text".into(),
                    confidence: 0.8,
                    kind: OcrTextKind::Recognized,
                },
                OcrTextBlock {
                    bounds: None,
                    text: "band:5,5,15,15".into(),
                    confidence: 0.0,
                    kind: OcrTextKind::BandDiagnostic,
                },
            ],
        };
        assert!(
            result
                .into_text_lines()
                .unwrap_err()
                .to_string()
                .contains("diagnostic")
        );
    }

    /// 文字列の接頭辞ではなく種別で診断を判別する。
    #[test]
    fn document_lines_keep_literal_band_text_and_explicit_mock() {
        for kind in [OcrTextKind::Recognized, OcrTextKind::Mock] {
            let result = OcrResult {
                source_id: "fixture:literal".into(),
                blocks: vec![OcrTextBlock {
                    bounds: None,
                    text: " band: a musical group ".into(),
                    confidence: 0.7,
                    kind,
                }],
            };
            let expected = match kind {
                OcrTextKind::Recognized => " band: a musical group ",
                _ => "band: a musical group",
            };
            assert_eq!(result.into_text_lines().unwrap(), [expected]);
        }
    }
}
