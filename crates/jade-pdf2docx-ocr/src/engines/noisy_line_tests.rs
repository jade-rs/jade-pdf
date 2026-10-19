use super::*;
use crate::engines::line_inference::LineInference;

/// 大きい入力容量で行検出・ページ予算への接続だけを確認する。
#[derive(Default)]
struct RecordingInference {
    /// 実際に推論へ渡された行数。
    calls: usize,
}

impl LineInference for RecordingInference {
    /// 合成行は分割せずに入力できる容量を返す。
    fn input_dimensions(&mut self, _: &Path, _: &Path) -> Result<(usize, usize), String> {
        Ok((4096, 24))
    }

    /// モデルの精度から独立した行順を返す。
    fn recognize(
        &mut self,
        _: &Path,
        _: &Path,
        _: &[u8],
        _: usize,
        _: usize,
    ) -> Result<RecognizeResult, String> {
        self.calls += 1;
        Ok(RecognizeResult {
            text: format!("line{}", self.calls),
            mean_confidence: 0.9,
        })
    }
}

/// 汚れで結合していた5物理行を別々に渡し、新しく検出した行も予算に数える。
#[test]
fn noisy_gaps_preserve_physical_rows_and_page_budget() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("noisy.ppm");
    let (width, height) = (400, 250);
    let mut rgb = vec![255; width * height * 3];
    for top in [10, 55, 100, 145, 190] {
        for y in top..top + 30 {
            for x in (40..280).step_by(30) {
                rgb[(y * width + x) * 3..(y * width + x + 20) * 3].fill(0);
            }
        }
    }
    for top in [130, 175] {
        for y in top..top + 15 {
            let x = 10 + y % 4 * 15;
            rgb[(y * width + x) * 3..(y * width + x + 8) * 3].fill(0);
        }
    }
    let mut ppm = b"P6\n400 250\n255\n".to_vec();
    ppm.extend(rgb);
    std::fs::write(&path, ppm).unwrap();
    let request = OcrRequest::new("noisy-gaps")
        .with_image_path(path)
        .with_parseq_paths("unused.onnx", "unused.yaml");
    let mut infer = RecordingInference::default();
    let OcrPageContent::PositionedText(lines) = NdlOcrLiteEngine
        .recognize_page_with(&request, &mut infer)
        .unwrap()
    else {
        panic!("positioned lines required")
    };
    assert_eq!(lines.len(), 5);
    assert_eq!(infer.calls, 5);
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(line.text, format!("line{}", i + 1));
        let top = [10, 55, 100, 145, 190][i];
        assert!(line.bounds[1] <= top && line.bounds[3] >= top + 30);
    }
    let request = request.with_text_limits(OcrTextLimits {
        max_bytes: 1000,
        max_lines: 4,
    });
    assert!(matches!(
        NdlOcrLiteEngine.recognize_page_with(&request, &mut RecordingInference::default()),
        Err(OcrError::ResourceLimit(_))
    ));
}

/// 短い字形部品を余分な一行として推論やページ予算に数えない。
#[test]
fn short_glyph_fragment_stays_with_its_body() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mixed-height.ppm");
    let (width, height) = (400, 210);
    let mut rgb = vec![255; width * height * 3];
    for [x0, y0, x1, y1] in [
        [40, 10, 280, 40],
        [40, 55, 280, 85],
        [100, 100, 200, 115],
        [40, 145, 280, 175],
    ] {
        for y in y0..y1 {
            rgb[(y * width + x0) * 3..(y * width + x1) * 3].fill(0);
        }
    }
    for y in 115..145 {
        let x = 10 + y % 4 * 15;
        rgb[(y * width + x) * 3..(y * width + x + 8) * 3].fill(0);
    }
    let mut ppm = b"P6\n400 210\n255\n".to_vec();
    ppm.extend(rgb);
    std::fs::write(&path, ppm).unwrap();
    let request = OcrRequest::new("short-glyph-fragment")
        .with_image_path(path)
        .with_parseq_paths("unused.onnx", "unused.yaml")
        .with_text_limits(OcrTextLimits {
            max_bytes: 1000,
            max_lines: 3,
        });
    let mut infer = RecordingInference::default();
    let result = NdlOcrLiteEngine.recognize_page_with(&request, &mut infer);
    assert!(result.is_ok(), "{result:?}");
    let OcrPageContent::PositionedText(lines) = result.unwrap() else {
        panic!("positioned lines required")
    };
    assert_eq!(lines.len(), 3);
    assert_eq!(infer.calls, 3);
    assert_eq!(lines[2].text, "line3");
    assert!(lines[2].bounds[0] <= 40 && lines[2].bounds[2] >= 280);
    assert!(lines[2].bounds[1] <= 100 && lines[2].bounds[3] >= 175);
}
