use crate::{OcrError, OcrRequest, OcrResult};

/// OCR エンジン抽象。
pub trait OcrEngine {
    /// リクエストを受け取り OCR 結果を返す。
    fn recognize(&self, request: &OcrRequest) -> Result<OcrResult, OcrError>;
}
