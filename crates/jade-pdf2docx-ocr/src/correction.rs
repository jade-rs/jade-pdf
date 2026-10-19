use crate::{OcrTextBlock, OcrTextKind};
use jade_pdf2docx_ocr_correction::{CorrectionError, CorrectionReport, Corrector};

impl OcrTextBlock {
    /// 認識本文だけの補正候補を返す。本文・座標・信頼度は変更しない。
    /// mock・診断は対象外。行の信頼度検証と処理量制限は専用crateへ委ねる。
    pub fn suggest_corrections(
        &self,
        corrector: &Corrector,
    ) -> Result<Option<CorrectionReport>, CorrectionError> {
        if self.kind != OcrTextKind::Recognized {
            return Ok(None);
        }
        corrector.analyze(&self.text, self.confidence).map(Some)
    }
}
