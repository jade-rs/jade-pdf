//! ONNXの有無にかかわらず行認識と入力寸法の契約を共有する。
use ndlocr_lite_rs::infer::parseq::RecognizeResult;
use std::path::Path;

/// 行分割に実際のモデル寸法を渡し、同じセッションで本文を認識する。
pub(crate) trait LineInference {
    /// 必要時だけロードし、モデルの入力幅と高さを返す。
    fn input_dimensions(&mut self, model: &Path, charset: &Path) -> Result<(usize, usize), String>;
    /// 横書き行断片を認識し、後処理前の本文と信頼度を返す。
    fn recognize(
        &mut self,
        model: &Path,
        charset: &Path,
        rgb: &[u8],
        width: usize,
        height: usize,
    ) -> Result<RecognizeResult, String>;
}

#[cfg(feature = "onnx")]
/// ONNXが有効な場合の、ページ内でモデルを再利用する推論器。
pub(crate) type InferenceSession = crate::PageOcrSession;

#[cfg(not(feature = "onnx"))]
/// ONNXを含まない構成で、本文候補の認識時だけ無効化エラーを返す。
#[derive(Default)]
pub(crate) struct InferenceSession {
    /// 空の状態を持つ推論器。モデルは保持しない。
    _private: (),
}

#[cfg(not(feature = "onnx"))]
impl LineInference for InferenceSession {
    /// ONNXを有効にしたビルドが必要であることを返す。
    fn input_dimensions(
        &mut self,
        _model: &Path,
        _charset: &Path,
    ) -> Result<(usize, usize), String> {
        Err("onnx feature is disabled. Rebuild with `--features onnx`.".into())
    }

    /// ONNXを有効にしたビルドが必要であることを返す。
    fn recognize(
        &mut self,
        _model: &Path,
        _charset: &Path,
        _rgb: &[u8],
        _width: usize,
        _height: usize,
    ) -> Result<RecognizeResult, String> {
        Err("onnx feature is disabled. Rebuild with `--features onnx`.".into())
    }
}
