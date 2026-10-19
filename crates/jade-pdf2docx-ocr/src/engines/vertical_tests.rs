use super::*;
use crate::{OcrTextDirection, engines::line_inference::LineInference};

/// 回転された入力の寸法・画素を記録し、ONNXなしで接続を検証する。
#[derive(Default)]
struct RecordingInference {
    /// 推論した断片の寸法。
    dimensions: Vec<(usize, usize)>,
    /// 指定した回で推論を失敗させる。
    fail_at: Option<usize>,
}

impl LineInference for RecordingInference {
    /// 大きな入力寸法を返し、短い合成列を分割しない。
    fn input_dimensions(&mut self, _: &Path, _: &Path) -> Result<(usize, usize), String> {
        Ok((4096, 24))
    }
    /// 呼出順を本文へ残し、縦列の回転と失敗の伝播を確認する。
    fn recognize(
        &mut self,
        _: &Path,
        _: &Path,
        _: &[u8],
        w: usize,
        h: usize,
    ) -> Result<RecognizeResult, String> {
        self.dimensions.push((w, h));
        let n = self.dimensions.len();
        if self.fail_at == Some(n) {
            return Err("deliberate failure".into());
        }
        Ok(RecognizeResult {
            text: format!("column{n}"),
            mean_confidence: 0.9,
        })
    }
}

/// 元画素の縦列を回して推論するが、返す矩形は元画像上の右→左順を維持する。
#[test]
fn explicit_vertical_rotates_columns_and_keeps_original_bounds() {
    let mut infer = RecordingInference::default();
    let rgb = vec![0; 100 * 200 * 3];
    let blocks = recognize_lines_from_bands(
        &rgb,
        100,
        200,
        &[[70, 10, 90, 180], [20, 20, 40, 170]],
        args(OcrTextLimits::unlimited()),
        &mut infer,
    )
    .unwrap();
    assert_eq!(infer.dimensions, [(170, 20), (150, 20)]);
    assert_eq!(
        blocks.iter().map(|b| b.bounds.unwrap()).collect::<Vec<_>>(),
        [[70, 10, 90, 180], [20, 20, 40, 170]]
    );
    assert_eq!(
        blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
        ["column1", "column2"]
    );
}

/// 先行する列が成功しても、容量超過や途中失敗で部分本文を公開しない。
#[test]
fn vertical_budget_and_failure_do_not_return_partial_columns() {
    for (limits, fail_at, calls) in [
        (
            OcrTextLimits {
                max_bytes: 100,
                max_lines: 0,
            },
            None,
            0,
        ),
        (
            OcrTextLimits {
                max_bytes: 100,
                max_lines: 1,
            },
            None,
            1,
        ),
        (OcrTextLimits::unlimited(), Some(2), 2),
    ] {
        let mut infer = RecordingInference {
            fail_at,
            ..Default::default()
        };
        let result = recognize_lines_from_bands(
            &vec![0; 20 * 100 * 3],
            20,
            100,
            &[[0, 0, 20, 50], [0, 50, 20, 100]],
            args(limits),
            &mut infer,
        );
        assert!(result.is_err());
        assert_eq!(infer.dimensions.len(), calls);
    }
}

/// 方向だけを明示し、通常経路と同じ予算・信頼度・失敗契約を使用する。
fn args(text_limits: OcrTextLimits) -> BandRecognizeArgs<'static> {
    BandRecognizeArgs {
        text_limits,
        model_path: Path::new("unused.onnx"),
        charset_path: Path::new("unused.yaml"),
        failure_policy: None,
        min_confidence: Some(0.5),
        threshold: 220,
        direction: OcrTextDirection::Vertical,
    }
}

/// 領域外のインクを除き、領域内の列座標へ元画像のoffsetを一度だけ戻す。
#[test]
fn vertical_region_restores_original_page_coordinates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("region.ppm");
    let mut rgb = vec![255; 100 * 200 * 3];
    for [x0, y0, x1, y1] in [[70, 20, 90, 180], [20, 20, 40, 180], [0, 0, 5, 200]] {
        for y in y0..y1 {
            for x in x0..x1 {
                rgb[(y * 100 + x) * 3..(y * 100 + x + 1) * 3].fill(0);
            }
        }
    }
    let mut ppm = b"P6\n100 200\n255\n".to_vec();
    ppm.extend(rgb);
    std::fs::write(&path, ppm).unwrap();
    let request = OcrRequest::new("vertical-region")
        .with_image_path(path)
        .with_image_region([10, 10, 95, 190])
        .with_text_direction(OcrTextDirection::Vertical)
        .with_parseq_paths("unused.onnx", "unused.yaml");
    let mut infer = RecordingInference::default();
    let OcrPageContent::PositionedText(lines) = NdlOcrLiteEngine
        .recognize_page_with(&request, &mut infer)
        .unwrap()
    else {
        panic!("positioned columns required");
    };
    // 列検出器の左右1px・上下2pxの文脈余白を含む元画像座標。
    assert_eq!(
        lines.iter().map(|line| line.bounds).collect::<Vec<_>>(),
        [[69, 18, 91, 182], [19, 18, 41, 182]]
    );
    assert_eq!(infer.dimensions, [(164, 22), (164, 22)]);
}
