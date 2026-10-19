use crate::{OcrError, OcrRequest, OcrResult, engine::OcrEngine};

/// 現在のデフォルト実装（段階導入用のスタブ）。
#[derive(Debug, Default)]
pub struct StubOcrEngine;

impl OcrEngine for StubOcrEngine {
    fn recognize(&self, request: &OcrRequest) -> Result<OcrResult, OcrError> {
        request.validate()?;
        Ok(OcrResult {
            source_id: request.source_id.clone(),
            blocks: Vec::new(),
        })
    }
}
