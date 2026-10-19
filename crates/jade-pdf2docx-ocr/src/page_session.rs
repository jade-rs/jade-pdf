//! 変換内の複数ページ・領域で、同じPARSeqモデルを共用する。
use crate::engines::line_inference::LineInference;
use crate::{NdlOcrLiteEngine, OcrError, OcrPageContent, OcrPageTimings, OcrRequest};
use ndlocr_lite_rs::infer::{
    cached::ParseqSession,
    parseq::{ParseqOrientation, RecognizeResult},
};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 同じモデルと文字セットの推論セッションを、最初の本文認識から保持する。
///
/// パスの組が変わると旧セッションを破棄する。同じパスのファイルを書き換えた場合は
/// 新しいインスタンスを作る。白紙だけの要求ではモデルをロードしない。
#[derive(Default)]
pub struct PageOcrSession {
    /// 現在のパスの組に対応するモデル。保持するモデルは最大一つ。
    cached: Option<CachedModel>,
}

/// ロード済みモデルと、セッションを選択するパスの組。
struct CachedModel {
    /// ONNXモデルの入力パス。
    model: PathBuf,
    /// 文字セットの入力パス。
    charset: PathBuf,
    /// 後処理前の本文を返す推論セッション。
    session: ParseqSession,
}

impl PageOcrSession {
    /// ページごとの本文予算と検証を適用し、モデルだけを次の要求へ再利用する。
    pub fn recognize_page(&mut self, request: &OcrRequest) -> Result<OcrPageContent, OcrError> {
        NdlOcrLiteEngine.recognize_page_with(request, self)
    }

    /// 通常と同じ認識結果に、成功した要求の内訳を添える。失敗時は従来のエラーを返す。
    pub fn recognize_page_with_timings(
        &mut self,
        request: &OcrRequest,
    ) -> Result<(OcrPageContent, OcrPageTimings), OcrError> {
        let start = Instant::now();
        let mut timings = OcrPageTimings::default();
        let content = NdlOcrLiteEngine.recognize_page_with(
            request,
            &mut TimedInference {
                session: self,
                timings: &mut timings,
            },
        )?;
        timings.total = start.elapsed();
        timings.processing = timings.total - timings.model_load - timings.inference;
        Ok((content, timings))
    }

    /// パスの組が変わった場合だけモデルを読み込み、読込の有無を返す。
    fn ensure_model(&mut self, model: &Path, charset: &Path) -> Result<bool, String> {
        if !self
            .cached
            .as_ref()
            .is_some_and(|cached| cached.model == model && cached.charset == charset)
        {
            self.cached = None;
            let session = ParseqSession::load(model, charset).map_err(|error| error.to_string())?;
            self.cached = Some(CachedModel {
                model: model.into(),
                charset: charset.into(),
                session,
            });
            return Ok(true);
        }
        Ok(false)
    }

    /// ロード済みモデルで、反復文字や空白を削らず1断片を認識する。
    fn infer_cached(
        &self,
        rgb: &[u8],
        width: usize,
        height: usize,
    ) -> Result<RecognizeResult, String> {
        self.cached
            .as_ref()
            .expect("session loaded")
            .session
            .recognize_rgb_u8_unprocessed_with_orientation(
                rgb,
                width,
                height,
                ParseqOrientation::Horizontal,
            )
            .map_err(|error| error.to_string())
    }
}

impl LineInference for PageOcrSession {
    /// 最初の本文候補でだけロードし、ONNXの実寸法を参照する。
    fn input_dimensions(&mut self, model: &Path, charset: &Path) -> Result<(usize, usize), String> {
        self.ensure_model(model, charset)?;
        Ok(self
            .cached
            .as_ref()
            .expect("session loaded")
            .session
            .input_dimensions())
    }

    /// 単発と複数ページの双方でロード済みモデルを再利用する。
    fn recognize(
        &mut self,
        model: &Path,
        charset: &Path,
        rgb: &[u8],
        width: usize,
        height: usize,
    ) -> Result<RecognizeResult, String> {
        self.ensure_model(model, charset)?;
        self.infer_cached(rgb, width, height)
    }
}

/// 寸法取得時の遅延ロードと本文推論を個別に計測する。
struct TimedInference<'a> {
    /// 呼出側が保持するモデルセッション。
    session: &'a mut PageOcrSession,
    /// 成功した要求の計測値。
    timings: &'a mut OcrPageTimings,
}

impl LineInference for TimedInference<'_> {
    /// モデルのロード時間だけを処理時間から分離する。
    fn input_dimensions(&mut self, model: &Path, charset: &Path) -> Result<(usize, usize), String> {
        let start = Instant::now();
        if self.session.ensure_model(model, charset)? {
            self.timings.model_load += start.elapsed();
            self.timings.model_load_count += 1;
        }
        self.session.input_dimensions(model, charset)
    }

    /// 本文推論の時間と呼出回数を保持する。
    fn recognize(
        &mut self,
        model: &Path,
        charset: &Path,
        rgb: &[u8],
        width: usize,
        height: usize,
    ) -> Result<RecognizeResult, String> {
        self.input_dimensions(model, charset)?;
        let start = Instant::now();
        let result = self.session.infer_cached(rgb, width, height);
        self.timings.inference += start.elapsed();
        self.timings.inference_count += 1;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mockの計測では推論・モデル読込を行わず、全時間をその他処理へ分類する。
    #[test]
    fn measured_mock_has_no_model_or_inference_cost() {
        let request = OcrRequest::new("mock").with_mock_lines(["abc"]);
        let mut session = PageOcrSession::default();
        let (content, timings) = session.recognize_page_with_timings(&request).unwrap();
        assert_eq!(content, session.recognize_page(&request).unwrap());
        assert_eq!(timings.model_load_count, 0);
        assert_eq!(timings.inference_count, 0);
        assert_eq!(timings.model_load, std::time::Duration::ZERO);
        assert_eq!(timings.inference, std::time::Duration::ZERO);
        assert_eq!(timings.processing, timings.total);
    }

    /// 白紙とmockはモデルファイルを必要とせず、セッションも保持しない。
    #[test]
    fn blank_and_mock_requests_do_not_load_models() {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("white.ppm");
        std::fs::write(&image, b"P6\n1 1\n255\n\xff\xff\xff").unwrap();
        let mut session = PageOcrSession::default();
        let request = OcrRequest::new("white")
            .with_image_path(&image)
            .with_parseq_paths(
                directory.path().join("missing.onnx"),
                directory.path().join("missing.yaml"),
            );
        assert!(matches!(
            session.recognize_page(&request),
            Ok(OcrPageContent::Blank(_))
        ));
        assert!(session.cached.is_none());
        let (_, timings) = session.recognize_page_with_timings(&request).unwrap();
        assert_eq!(timings.model_load_count, 0);
        assert_eq!(timings.inference_count, 0);
        assert_eq!(timings.processing, timings.total);
        assert!(
            session
                .recognize_page(&OcrRequest::new("mock").with_mock_lines(["abc"]))
                .is_ok()
        );
        assert!(session.cached.is_none());
    }
}
