//! 任意のページ計測で、モデル読込と認識呼出しの時間を分離する。
use std::time::Duration;

/// 成功した1要求の実測時間。通常の認識APIでは計測しない。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OcrPageTimings {
    /// 要求開始から結果の検証完了までの時間。
    pub total: Duration,
    /// この要求でモデルと文字セットを読み込んだ時間。再利用時はゼロ。
    pub model_load: Duration,
    /// 画像断片の認識API内の累積時間。テンソル準備・推論・復号も含む。
    pub inference: Duration,
    /// 残りの時間。画像読込・行検出・切り出し・結合・検証・計測の経費を含む。
    pub processing: Duration,
    /// この要求でモデルを読み込んだ回数。
    pub model_load_count: usize,
    /// この要求で認識APIを呼び出した断片数。
    pub inference_count: usize,
}
