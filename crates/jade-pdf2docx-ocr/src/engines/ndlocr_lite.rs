use super::line_inference::InferenceSession;
use crate::{
    OcrError, OcrPageContent, OcrRequest, OcrResult, OcrTextBlock, OcrTextBudget, OcrTextDirection,
    OcrTextKind, OcrTextLimits, ParseqFailurePolicy, VerifiedBlankPage, engine::OcrEngine,
};
use ndlocr_lite_rs::infer::parseq::{RecognizeResult, sanitize_recognized_text};
use ndlocr_lite_rs::io::load_rgb_u8;
use ndlocr_lite_rs::pipeline::background::normalize_chromatic_panels;
use ndlocr_lite_rs::pipeline::crop::{BBox, crop_rgb_u8};
#[cfg(test)]
use ndlocr_lite_rs::pipeline::line_chunks::split_line;
use ndlocr_lite_rs::pipeline::line_chunks::{
    LineChunk, crop_line_chunk_rgb_u8, split_line_for_input,
    split_rotated_vertical_column_for_input,
};
use ndlocr_lite_rs::pipeline::line_segment::detect_textline_bands_naive;
use ndlocr_lite_rs::pipeline::paper_background::normalize_paper_shading;
use ndlocr_lite_rs::pipeline::vertical_segment::{
    detect_vertical_textline_columns, rotate_vertical_column_rgb_u8,
};
use std::path::Path;

#[cfg(test)]
#[path = "noisy_line_tests.rs"]
mod noisy_line_tests;

#[cfg(test)]
#[path = "vertical_tests.rs"]
mod vertical_tests;

/// モデル入力寸法と後処理前の認識結果を返す共用推論器。
type Inference<'a> = dyn super::line_inference::LineInference + 'a;

/// テキスト行バンドを PARSeq で読み取るときのモデルパス・失敗時ポリシー。
struct BandRecognizeArgs<'a> {
    /// 列の回転を明示し、寸法比から推測しない。
    direction: OcrTextDirection,
    /// ページへ割り当てられた本文の残り容量。
    text_limits: OcrTextLimits,
    /// ONNX モデルパス。
    model_path: &'a Path,
    /// 文字セット YAML パス。
    charset_path: &'a Path,
    /// PARSeq 失敗時の扱い（未指定時はエラー）。
    failure_policy: Option<ParseqFailurePolicy>,
    /// この信頼度未満の認識結果を失敗扱いにする。
    min_confidence: Option<f32>,
    /// 行検出と分割に共用するインクの閾値。
    threshold: u8,
}

/// `ndlocr-lite-rs` の行検出と PARSeq 呼び出しを仲介するエンジン。
#[derive(Debug, Default)]
pub struct NdlOcrLiteEngine;

/// 画像解析時の本文候補と全白の証明。
#[derive(Debug)]
enum ImagePage {
    /// 行認識を完了したブロック列。
    Text(Vec<OcrTextBlock>),
    /// 全成分が白の画像。
    Blank(VerifiedBlankPage),
}

/// フィルタ後の本文または白紙を、従来の本文専用APIへ引き渡す前に区別する。
enum RecognizedPage {
    /// 信頼度と由来を保持した認識結果。
    Text(OcrResult),
    /// 画像全体を検証した白紙。
    Blank(VerifiedBlankPage),
}

impl NdlOcrLiteEngine {
    /// 画像を一度だけ読み、本文の失敗と全白を区別する。
    pub(crate) fn recognize_page(&self, request: &OcrRequest) -> Result<OcrPageContent, OcrError> {
        self.recognize_page_with(request, &mut InferenceSession::default())
    }

    /// 推論器を共用しつつ、ページごとの検証と本文予算を適用する。
    pub(crate) fn recognize_page_with(
        &self,
        request: &OcrRequest,
        infer: &mut Inference<'_>,
    ) -> Result<OcrPageContent, OcrError> {
        match self.recognize_content_with(request, true, infer)? {
            RecognizedPage::Text(result) => result.into_page_content(),
            RecognizedPage::Blank(proof) => Ok(OcrPageContent::Blank(proof)),
        }
    }

    /// 画像の認識結果または明示mockを、由来・信頼度を保持して返す。
    fn recognize_content(
        &self,
        request: &OcrRequest,
        allow_blank: bool,
    ) -> Result<RecognizedPage, OcrError> {
        self.recognize_content_with(request, allow_blank, &mut InferenceSession::default())
    }

    /// 単発とセッションの双方へ同じ本文検証を適用する。
    fn recognize_content_with(
        &self,
        request: &OcrRequest,
        allow_blank: bool,
        infer: &mut Inference<'_>,
    ) -> Result<RecognizedPage, OcrError> {
        if allow_blank {
            request.validate_fields()?;
        } else {
            request.validate()?;
        }
        let blocks = if let Some(page) = parse_lines_from_image_path_with(request, infer)? {
            match page {
                ImagePage::Text(blocks) => blocks,
                ImagePage::Blank(proof) => return Ok(RecognizedPage::Blank(proof)),
            }
        } else if let Some(lines) = parse_mock_lines_from_request(request) {
            lines.into_iter().map(mock_text_block).collect()
        } else {
            return Err(OcrError::EngineFailure(
                "ndlocr-lite bridge expects request.with_image_path(...), request.with_mock_lines(...), or source_id in `mock:line1|line2` format".to_string(),
            ));
        };
        let mut budget =
            OcrTextBudget::new(request.text_limits.unwrap_or(OcrTextLimits::unlimited()));
        for block in &blocks {
            budget.reserve_line(&block.text)?;
        }
        let blocks = apply_output_filters(blocks, request)?;
        if blocks.is_empty() {
            return Err(OcrError::EngineFailure(
                "no OCR lines remained after applying output filters".to_string(),
            ));
        }
        Ok(RecognizedPage::Text(OcrResult {
            source_id: request.source_id.clone(),
            blocks,
        }))
    }
}

impl OcrEngine for NdlOcrLiteEngine {
    /// 本文専用の従来APIでは、白紙も空の認識成功には変更しない。
    fn recognize(&self, request: &OcrRequest) -> Result<OcrResult, OcrError> {
        match self.recognize_content(request, false)? {
            RecognizedPage::Text(result) => Ok(result),
            RecognizedPage::Blank(_) => Err(OcrError::EngineFailure(
                "verified blank page has no OCR text blocks".into(),
            )),
        }
    }
}

/// 明示mockの本文を、推論結果と区別できるブロックにする。
fn mock_text_block(text: String) -> OcrTextBlock {
    OcrTextBlock {
        bounds: None,
        text,
        confidence: 1.0,
        kind: OcrTextKind::Mock,
    }
}

/// 旧形式の明示mock入力を正規化する。
fn parse_mock_lines_from_source_id(source_id: &str) -> Option<Vec<String>> {
    let payload = source_id.strip_prefix("mock:")?;
    Some(
        payload
            .split('|')
            .map(sanitize_recognized_text)
            .filter(|line| !line.is_empty())
            .collect(),
    )
}

/// 画像から行を検出して認識する。モデル未指定で座標を本文として返さない。
#[cfg(test)]
fn parse_lines_from_image_path(request: &OcrRequest) -> Result<Option<ImagePage>, OcrError> {
    parse_lines_from_image_path_with(request, &mut InferenceSession::default())
}

/// 画像を検証し、注入された推論器で検出行を認識する。
fn parse_lines_from_image_path_with(
    request: &OcrRequest,
    infer: &mut Inference<'_>,
) -> Result<Option<ImagePage>, OcrError> {
    let Some(image_path) = request.image_path.as_ref() else {
        return Ok(None);
    };
    let (Some(model_path), Some(charset_path)) = (
        request.parseq_model_path.as_ref(),
        request.parseq_charset_path.as_ref(),
    ) else {
        return Err(OcrError::InvalidRequest(
            "image OCR requires PARSeq model and charset paths".into(),
        ));
    };
    let mut image = load_rgb_u8(image_path).map_err(|error| {
        OcrError::EngineFailure(format!(
            "failed to load image from `{}`: {error}",
            image_path.display()
        ))
    })?;
    if let Some([x0, y0, x1, y1]) = request.image_region {
        let cropped = crop_rgb_u8(
            &image.data,
            image.width,
            image.height,
            BBox::new(x0, y0, x1, y1),
        )
        .map_err(|error| OcrError::InvalidRequest(format!("invalid image region: {error}")))?;
        image.data = cropped.data;
        image.width = cropped.width;
        image.height = cropped.height;
    }
    if let Some(proof) = VerifiedBlankPage::from_rgb8(&image.data, image.width, image.height) {
        return Ok(Some(ImagePage::Blank(proof)));
    }
    let threshold = request.binarize_threshold.unwrap_or(220);
    if let std::borrow::Cow::Owned(prepared) =
        normalize_chromatic_panels(&image.data, image.width, image.height)
            .map_err(OcrError::EngineFailure)?
    {
        image.data = prepared;
    }
    if request.paper_shading
        && let std::borrow::Cow::Owned(prepared) =
            normalize_paper_shading(&image.data, image.width, image.height)
                .map_err(OcrError::EngineFailure)?
    {
        image.data = prepared;
    }
    let bands = match request.text_direction {
        OcrTextDirection::Horizontal => {
            detect_textline_bands_naive(&image.data, image.width, image.height, threshold)
        }
        OcrTextDirection::Vertical => {
            detect_vertical_textline_columns(&image.data, image.width, image.height, threshold)
                .map_err(OcrError::EngineFailure)?
        }
    };
    if bands.is_empty() {
        return Err(OcrError::EngineFailure(format!(
            "no textline bands detected from `{}` with threshold {threshold}",
            image_path.display()
        )));
    }
    recognize_lines_from_bands(
        &image.data,
        image.width,
        image.height,
        &bands,
        BandRecognizeArgs {
            direction: request.text_direction,
            text_limits: request.text_limits.unwrap_or(OcrTextLimits::unlimited()),
            model_path,
            charset_path,
            failure_policy: request.parseq_failure_policy,
            min_confidence: request.parseq_min_confidence,
            threshold,
        },
        infer,
    )
    .map(|mut blocks| {
        let [offset_x, offset_y, _, _] = request.image_region.unwrap_or([0; 4]);
        for block in &mut blocks {
            if let Some([x0, y0, x1, y1]) = block.bounds {
                block.bounds = Some([x0 + offset_x, y0 + offset_y, x1 + offset_x, y1 + offset_y]);
            }
        }
        Some(ImagePage::Text(blocks))
    })
}

/// 推論結果を検証し、モデルの信頼度を保持した本文ブロックへ変換する。
fn recognized_text_block(
    result: RecognizeResult,
    minimum: Option<f32>,
) -> Result<OcrTextBlock, String> {
    let confidence = result.mean_confidence;
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(format!("invalid recognition confidence: {confidence}"));
    }
    if let Some(minimum) = minimum
        && confidence < minimum
    {
        return Err(format!(
            "below minimum confidence: got {confidence:.4}, required {minimum:.4}"
        ));
    }
    let text = result.text;
    if text.trim().is_empty() {
        return Err("no non-empty text recognized".into());
    }
    Ok(OcrTextBlock {
        bounds: None,
        text,
        confidence,
        kind: OcrTextKind::Recognized,
    })
}

/// 行ごとの推論失敗をエラーまたは明示的な診断ブロックへ変換する。
fn recognize_lines_from_bands(
    rgb: &[u8],
    width: usize,
    height: usize,
    bands: &[[usize; 4]],
    parseq: BandRecognizeArgs<'_>,
    infer: &mut Inference<'_>,
) -> Result<Vec<OcrTextBlock>, OcrError> {
    let mut blocks = Vec::new();
    let mut budget = OcrTextBudget::new(parseq.text_limits);
    for [x0, y0, x1, y1] in bands.iter().copied() {
        budget.check_line_bytes(1)?;
        let crop = crop_rgb_u8(rgb, width, height, BBox::new(x0, y0, x1, y1)).map_err(|error| {
            OcrError::EngineFailure(format!(
                "failed to crop OCR band [{x0},{y0},{x1},{y1}]: {error}"
            ))
        })?;
        let crop = if parseq.direction == OcrTextDirection::Vertical {
            rotate_vertical_column_rgb_u8(&crop.data, crop.width, crop.height)
                .map_err(OcrError::EngineFailure)?
        } else {
            crop
        };
        let recognized = recognize_band(
            &crop.data,
            crop.width,
            crop.height,
            &parseq,
            &mut budget,
            infer,
        );
        match recognized {
            Ok(mut block) => {
                block.bounds = Some([x0, y0, x1, y1]);
                blocks.push(block);
            }
            Err(error @ OcrError::ResourceLimit(_)) => return Err(error),
            Err(error) => match parseq
                .failure_policy
                .unwrap_or(ParseqFailurePolicy::FailFast)
            {
                ParseqFailurePolicy::FailFast => {
                    return Err(OcrError::EngineFailure(format!(
                        "failed to recognize OCR band [{x0},{y0},{x1},{y1}] with PARSeq: {error}"
                    )));
                }
                ParseqFailurePolicy::FallbackToBandLabel => {
                    let text = format!("band:{x0},{y0},{x1},{y1}");
                    budget.reserve_line(&text)?;
                    blocks.push(OcrTextBlock {
                        bounds: None,
                        text,
                        confidence: 0.0,
                        kind: OcrTextKind::BandDiagnostic,
                    });
                }
            },
        }
    }
    if blocks.is_empty() {
        return Err(OcrError::EngineFailure(
            "no textline bands to recognize".into(),
        ));
    }
    Ok(blocks)
}

/// 長い行を白列で分割して順番に認識し、最も低い区間信頼度を行へ残す。
fn recognize_band(
    rgb: &[u8],
    width: usize,
    height: usize,
    parseq: &BandRecognizeArgs<'_>,
    budget: &mut OcrTextBudget,
    infer: &mut Inference<'_>,
) -> Result<OcrTextBlock, OcrError> {
    budget.check_line_bytes(1)?;
    let (input_width, input_height) = infer
        .input_dimensions(parseq.model_path, parseq.charset_path)
        .map_err(OcrError::EngineFailure)?;
    let split = match parseq.direction {
        OcrTextDirection::Horizontal => split_line_for_input,
        OcrTextDirection::Vertical => split_rotated_vertical_column_for_input,
    };
    let chunks = split(
        rgb,
        width,
        height,
        parseq.threshold,
        input_width,
        input_height,
    )
    .map_err(OcrError::EngineFailure)?;
    recognize_chunks_with(
        rgb,
        width,
        height,
        chunks,
        parseq.min_confidence,
        budget,
        |rgb, width, height| {
            infer.recognize(parseq.model_path, parseq.charset_path, rgb, width, height)
        },
    )
}

/// 従来の保守的分割で、本文予算・途中失敗の単体テストを実行する。
#[cfg(test)]
fn recognize_band_with(
    rgb: &[u8],
    width: usize,
    height: usize,
    threshold: u8,
    minimum: Option<f32>,
    budget: &mut OcrTextBudget,
    infer: impl FnMut(&[u8], usize, usize) -> Result<RecognizeResult, String>,
) -> Result<OcrTextBlock, OcrError> {
    budget.check_line_bytes(1)?;
    let chunks = split_line(rgb, width, height, threshold).map_err(OcrError::EngineFailure)?;
    recognize_chunks_with(rgb, width, height, chunks, minimum, budget, infer)
}

/// 区間ごとの推論と検証を一括処理し、途中失敗で部分本文を返さない。
fn recognize_chunks_with(
    rgb: &[u8],
    width: usize,
    height: usize,
    chunks: Vec<LineChunk>,
    minimum: Option<f32>,
    budget: &mut OcrTextBudget,
    mut infer: impl FnMut(&[u8], usize, usize) -> Result<RecognizeResult, String>,
) -> Result<OcrTextBlock, OcrError> {
    let mut line = OcrTextBlock {
        bounds: None,
        text: String::new(),
        confidence: 1.0,
        kind: OcrTextKind::Recognized,
    };
    for (index, chunk) in chunks.iter().enumerate() {
        budget.check_line_bytes(line.text.len() as u64 + 1)?;
        let crop = crop_line_chunk_rgb_u8(rgb, width, height, &chunks, index)
            .map_err(|error| OcrError::EngineFailure(error.to_string()))?;
        let result = infer(&crop.data, crop.width, crop.height).map_err(OcrError::EngineFailure)?;
        let block = recognized_text_block(result, minimum).map_err(OcrError::EngineFailure)?;
        let insert_space = chunk.insert_space_before(&line.text, &block.text, height);
        let prefix = line.text.len() as u64 + u64::from(insert_space);
        budget.check_line_bytes(prefix + block.text.len() as u64)?;
        if insert_space {
            line.text.push(' ');
        }
        line.text.push_str(&block.text);
        line.confidence = line.confidence.min(block.confidence);
    }
    budget.reserve_line(&line.text)?;
    Ok(line)
}

/// 明示mock行または旧形式のmock source_idを正規化する。
fn parse_mock_lines_from_request(request: &OcrRequest) -> Option<Vec<String>> {
    if let Some(lines) = request.mock_lines.as_ref() {
        let normalized = lines
            .iter()
            .map(|line| sanitize_recognized_text(line))
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        if !normalized.is_empty() {
            return Some(normalized);
        }
    }
    parse_mock_lines_from_source_id(&request.source_id)
}

/// 本文の出力条件を適用し、診断が隠れる場合は結果全体を拒否する。
fn apply_output_filters(
    blocks: Vec<OcrTextBlock>,
    request: &OcrRequest,
) -> Result<Vec<OcrTextBlock>, OcrError> {
    let mut accepted = Vec::new();
    for block in blocks {
        let excluded = block.text.chars().count() < request.min_line_length.unwrap_or(0)
            || request
                .max_symbol_ratio
                .is_some_and(|limit| symbol_ratio(&block.text) > limit)
            || accepted.len() >= request.max_output_lines.unwrap_or(usize::MAX);
        if excluded {
            if block.kind == OcrTextKind::BandDiagnostic {
                return Err(OcrError::EngineFailure(
                    "output filters would hide an OCR band diagnostic".into(),
                ));
            }
        } else {
            accepted.push(block);
        }
    }
    Ok(accepted)
}

/// 空白を除く文字列の記号比率を求める。
fn symbol_ratio(line: &str) -> f32 {
    let mut total = 0usize;
    let mut symbols = 0usize;
    for ch in line.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if !ch.is_alphanumeric() {
            symbols += 1;
        }
    }
    if total == 0 {
        return 1.0;
    }
    symbols as f32 / total as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env, fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    /// 単語境界で分割したchunkだけ白列を除き、字形間分割・未分割行・画線を保持する。
    #[test]
    fn split_chunks_remove_only_white_outer_columns() {
        let mut rgb = vec![255; 64 * 2 * 3];
        for x in (2..22).step_by(2).chain((34..54).step_by(2)) {
            rgb[(64 + x) * 3] = 254;
        }
        rgb[(64 + 20) * 3 + 2] = 0;
        for (split, word_boundary) in [(true, true), (true, false), (false, false)] {
            let chunks = if split {
                vec![
                    LineChunk {
                        x0: 0,
                        x1: 32,
                        space_before: false,
                        gap_before: 0,
                    },
                    LineChunk {
                        x0: 32,
                        x1: 64,
                        space_before: word_boundary,
                        gap_before: 3,
                    },
                ]
            } else {
                vec![LineChunk {
                    x0: 0,
                    x1: 64,
                    space_before: false,
                    gap_before: 0,
                }]
            };
            let mut calls = 0;
            let mut budget = OcrTextBudget::new(OcrTextLimits::unlimited());
            recognize_chunks_with(
                &rgb,
                64,
                2,
                chunks,
                None,
                &mut budget,
                |data, width, height| {
                    let (x0, x1) = if !split {
                        (0, 64)
                    } else if !word_boundary {
                        (calls * 32, (calls + 1) * 32)
                    } else if calls == 0 {
                        (2, 21)
                    } else {
                        (34, 53)
                    };
                    assert_eq!((width, height), (x1 - x0, 2));
                    for y in 0..height {
                        assert_eq!(
                            &data[y * width * 3..(y + 1) * width * 3],
                            &rgb[(y * 64 + x0) * 3..(y * 64 + x1) * 3]
                        );
                    }
                    calls += 1;
                    Ok(RecognizeResult {
                        text: "text".into(),
                        mean_confidence: 1.0,
                    })
                },
            )
            .unwrap();
            assert_eq!(calls, if split { 2 } else { 1 });
        }
    }

    /// 白紙証明を指定領域へ限定し、画像外の切り出しを丸めず拒否する。
    #[test]
    fn region_content_ignores_external_ink_and_rejects_invalid_bounds() {
        let image_path = temp_ppm_path("region_white");
        write_test_ppm_with_band(&image_path, 20, 20, 0, 0, 4, 4);
        let request = OcrRequest::new("region")
            .with_image_path(&image_path)
            .with_parseq_paths("unused.onnx", "unused.yaml")
            .with_text_limits(OcrTextLimits {
                max_bytes: 0,
                max_lines: 0,
            })
            .with_image_region([10, 10, 20, 20]);
        assert!(matches!(
            NdlOcrLiteEngine.recognize_page(&request),
            Ok(OcrPageContent::Blank(_))
        ));
        for region in [
            [10, 10, 21, 20],
            [10, 10, 20, 21],
            [10, 10, 10, 20],
            [usize::MAX, 0, usize::MAX, 1],
        ] {
            let request = request.clone().with_image_region(region);
            assert!(matches!(
                NdlOcrLiteEngine.recognize_page(&request),
                Err(OcrError::InvalidRequest(_))
            ));
        }
        fs::remove_file(image_path).unwrap();
    }

    /// 全白の画像だけを白紙として返し、従来の本文専用APIでは拒否する。
    #[test]
    fn page_content_distinguishes_white_from_no_detected_bands() {
        let image_path = temp_ppm_path("verified_white_page");
        let request = OcrRequest::new("page")
            .with_image_path(&image_path)
            .with_parseq_paths("unused.onnx", "unused.yaml")
            .with_text_limits(OcrTextLimits {
                max_bytes: 0,
                max_lines: 0,
            });
        write_test_ppm_with_band(&image_path, 20, 20, 0, 0, 0, 0);
        assert!(matches!(
            NdlOcrLiteEngine.recognize_page(&request),
            Ok(OcrPageContent::Blank(_))
        ));
        assert!(NdlOcrLiteEngine.recognize(&request).is_err());
        // 1成分の微小差でも、行検出できない画像を白紙成功に変更しない。
        let ppm =
            fs::read_to_string(&image_path)
                .unwrap()
                .replacen("255 255 255", "254 255 255", 1);
        fs::write(&image_path, ppm).unwrap();
        let error = NdlOcrLiteEngine
            .recognize_page(&request)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no textline bands"), "{error}");
        fs::write(&image_path, b"broken image").unwrap();
        assert!(NdlOcrLiteEngine.recognize_page(&request).is_err());
        fs::remove_file(image_path).unwrap();
    }

    /// 推論器の信頼度を1.0で上書きせず保持する。
    #[test]
    fn recognized_block_preserves_model_confidence() {
        let block = recognized_text_block(
            ndlocr_lite_rs::infer::parseq::RecognizeResult {
                text: "recognized text".into(),
                mean_confidence: 0.625,
            },
            Some(0.5),
        )
        .unwrap();
        assert_eq!(block.confidence, 0.625);
        assert_eq!(block.kind, crate::OcrTextKind::Recognized);
    }

    /// 正当な反復数字・部分文字列・括弧・日本語を後処理で書き換えない。
    #[test]
    fn recognized_block_preserves_decoded_text() {
        for text in [
            "111111",
            "123123123",
            "ABABAB",
            "という。という。",
            "乙）",
            "単又は乙",
        ] {
            let block = recognized_text_block(
                RecognizeResult {
                    text: text.into(),
                    mean_confidence: 0.9,
                },
                Some(0.5),
            )
            .unwrap();
            assert_eq!(block.text, text);
        }
    }

    /// 非有限・範囲外・閾値未満・空本文を認識成功にしない。
    #[test]
    fn recognized_block_rejects_invalid_score_or_empty_text() {
        for (text, score, minimum) in [
            ("text", f32::NAN, None),
            ("text", f32::INFINITY, None),
            ("text", f32::NEG_INFINITY, None),
            ("text", -0.1, None),
            ("text", 1.1, None),
            ("text", 0.4, Some(0.5)),
            ("   ", 0.9, None),
        ] {
            assert!(
                recognized_text_block(
                    ndlocr_lite_rs::infer::parseq::RecognizeResult {
                        text: text.into(),
                        mean_confidence: score,
                    },
                    minimum,
                )
                .is_err(),
                "unexpected success: {text:?}, {score:?}"
            );
        }
    }

    /// 出力filterによって容量超過を隠して成功扱いにしない。
    #[test]
    fn output_filters_cannot_hide_excess_text() {
        let request = OcrRequest::new("budget:mock")
            .with_mock_lines(["first", "second"])
            .with_max_output_lines(1)
            .with_text_limits(crate::OcrTextLimits {
                max_bytes: 100,
                max_lines: 1,
            });
        let result = NdlOcrLiteEngine.recognize(&request);
        assert!(
            matches!(result, Err(OcrError::ResourceLimit(_))),
            "{result:?}"
        );
    }

    /// 和文句読点の字面余白では空白を補わず、明示的な広い間隔と英文は保つ。
    #[test]
    fn japanese_punctuation_bearing_is_not_an_inserted_space() {
        for (left, right, gap, expected) in [
            ("万円、", "支払", 10, "万円、支払"),
            ("完了。", "次", 10, "完了。次"),
            ("万円、", "支払", 16, "万円、 支払"),
            ("万円、", "next", 10, "万円、 next"),
            ("first", "second", 10, "first second"),
        ] {
            let rgb = (0..12)
                .flat_map(|_| {
                    (0..130).flat_map(|x| [if (60..60 + gap).contains(&x) { 255 } else { 0 }; 3])
                })
                .collect::<Vec<_>>();
            let mut results = [left, right].into_iter();
            let mut budget = OcrTextBudget::new(OcrTextLimits {
                max_bytes: expected.len() as u64 + 1,
                max_lines: 1,
            });
            let result = recognize_band_with(&rgb, 130, 12, 220, None, &mut budget, |_, _, _| {
                Ok(RecognizeResult {
                    text: results.next().unwrap().into(),
                    mean_confidence: 0.9,
                })
            })
            .unwrap();
            assert_eq!(result.text, expected);
            assert_eq!(budget.remaining().max_bytes, 0);
        }
    }

    /// 区間結合で空白を保持し、最も弱い区間の信頼度を行へ残す。
    #[test]
    fn chunked_line_uses_lowest_confidence_and_word_order() {
        let rgb = two_chunk_image();
        let mut results = [
            RecognizeResult {
                text: "first".into(),
                mean_confidence: 0.9,
            },
            RecognizeResult {
                text: "second".into(),
                mean_confidence: 0.6,
            },
        ]
        .into_iter();
        let mut budget = OcrTextBudget::new(OcrTextLimits::unlimited());
        let result = recognize_band_with(&rgb, 130, 10, 220, Some(0.5), &mut budget, |_, _, _| {
            Ok(results.next().expect("exactly two chunks"))
        })
        .unwrap();
        assert_eq!(result.text, "first second");
        assert_eq!(result.confidence, 0.6);
        assert_eq!(result.kind, OcrTextKind::Recognized);
        assert!(results.next().is_none());
    }

    /// 後半の区間が推論失敗・閾値未満・空本文でも前半だけの成功にしない。
    #[test]
    fn chunked_line_rejects_late_failure() {
        for late in [
            Err("late model failure".to_owned()),
            Ok(RecognizeResult {
                text: "second".into(),
                mean_confidence: 0.4,
            }),
            Ok(RecognizeResult {
                text: " ".into(),
                mean_confidence: 0.9,
            }),
        ] {
            let mut results = [
                Ok(RecognizeResult {
                    text: "first".into(),
                    mean_confidence: 0.9,
                }),
                late,
            ]
            .into_iter();
            let mut budget = OcrTextBudget::new(OcrTextLimits::unlimited());
            let result = recognize_band_with(
                &two_chunk_image(),
                130,
                10,
                220,
                Some(0.5),
                &mut budget,
                |_, _, _| results.next().expect("exactly two chunks"),
            );
            assert!(result.is_err(), "must reject partial text: {result:?}");
        }
    }

    /// 結合前に本文と挿入空白・行区切りを検査し、超過した行の容量を消費しない。
    #[test]
    fn chunks_respect_remaining_bytes_before_concatenation() {
        for (max_bytes, succeeds, expected_calls) in [(5, false, 1), (12, false, 2), (13, true, 2)]
        {
            let limits = OcrTextLimits {
                max_bytes,
                max_lines: 1,
            };
            let mut budget = OcrTextBudget::new(limits);
            let mut calls = 0;
            let result = recognize_band_with(
                &two_chunk_image(),
                130,
                10,
                220,
                None,
                &mut budget,
                |_, _, _| {
                    calls += 1;
                    Ok(RecognizeResult {
                        text: if calls == 1 { "first" } else { "second" }.into(),
                        mean_confidence: 0.9,
                    })
                },
            );
            assert_eq!(calls, expected_calls);
            if succeeds {
                assert_eq!(result.unwrap().text, "first second");
                assert_eq!(
                    budget.remaining(),
                    OcrTextLimits {
                        max_bytes: 0,
                        max_lines: 0
                    }
                );
            } else {
                assert!(matches!(result, Err(OcrError::ResourceLimit(_))));
                assert_eq!(budget.remaining(), limits);
            }
        }
    }

    /// 容量のない行を推論せず、部分本文や診断fallbackを返さない。
    #[test]
    fn exhausted_text_budget_skips_inference_even_with_fallback() {
        let parseq = BandRecognizeArgs {
            direction: OcrTextDirection::Horizontal,
            model_path: Path::new("missing.onnx"),
            charset_path: Path::new("missing.yaml"),
            failure_policy: Some(ParseqFailurePolicy::FallbackToBandLabel),
            min_confidence: None,
            threshold: 220,
            text_limits: OcrTextLimits {
                max_bytes: 100,
                max_lines: 0,
            },
        };
        let result = recognize_lines_from_bands(
            &two_chunk_image(),
            130,
            10,
            &[[0, 0, 130, 10]],
            parseq,
            &mut InferenceSession::default(),
        );
        assert!(matches!(result, Err(OcrError::ResourceLimit(_))));
    }

    /// 高さ10・幅130の行に、二つの区間を区切る単語空白を配置する。
    fn two_chunk_image() -> Vec<u8> {
        (0..10)
            .flat_map(|_| (0..130).flat_map(|x| [if (60..70).contains(&x) { 255 } else { 0 }; 3]))
            .collect()
    }

    /// モデルを指定しない画像入力を認識済み本文として返さない。
    #[test]
    fn image_without_parseq_model_is_not_recognized_text() {
        let image_path = temp_ppm_path("ndlocr_model_required");
        write_test_ppm_with_band(&image_path, 20, 20, 5, 5, 15, 15);
        let result = NdlOcrLiteEngine
            .recognize(&OcrRequest::new("fixture:image").with_image_path(&image_path));
        let _ = fs::remove_file(image_path);
        assert!(
            matches!(result, Err(OcrError::InvalidRequest(message)) if message.contains("PARSeq"))
        );
    }

    /// 明示的な座標ラベルのフォールバックに認識成功の信頼度を付けない。
    #[test]
    fn band_fallback_has_no_recognition_confidence() {
        let image_path = temp_ppm_path("ndlocr_diagnostic_confidence");
        write_test_ppm_with_band(&image_path, 20, 20, 5, 5, 15, 15);
        let result = NdlOcrLiteEngine.recognize(
            &OcrRequest::new("fixture:image")
                .with_image_path(&image_path)
                .with_parseq_paths("missing-model.onnx", "missing-charset.yaml")
                .with_parseq_failure_policy(ParseqFailurePolicy::FallbackToBandLabel),
        );
        let _ = fs::remove_file(image_path);
        let result = result.expect("explicit diagnostic fallback should be available");
        assert!(!result.blocks.is_empty());
        assert!(result.blocks.iter().all(|block| block.confidence == 0.0));
    }

    /// 出力条件で診断が消えて部分本文だけの成功結果にならない。
    #[test]
    fn output_filters_reject_hidden_diagnostics() {
        let request = OcrRequest::new("fixture:filters")
            .with_max_output_lines(1)
            .with_min_line_length(20)
            .with_max_symbol_ratio(0.0);
        let blocks = vec![
            mock_text_block("recognized text long enough".into()),
            OcrTextBlock {
                bounds: None,
                text: "band:5,5,15,15".into(),
                confidence: 0.0,
                kind: OcrTextKind::BandDiagnostic,
            },
        ];
        assert!(apply_output_filters(blocks, &request).is_err());
    }

    /// 診断ラベルにも最大行数の契約を適用する。
    #[test]
    fn diagnostic_blocks_do_not_exceed_max_output_lines() {
        let request = OcrRequest::new("fixture:limit").with_max_output_lines(1);
        let blocks = (0..2)
            .map(|_| OcrTextBlock {
                bounds: None,
                text: "band:5,5,15,15".into(),
                confidence: 0.0,
                kind: OcrTextKind::BandDiagnostic,
            })
            .collect();
        assert!(apply_output_filters(blocks, &request).is_err());
    }

    proptest::proptest! {
        /// 空白以外を含む任意の Unicode 本文を、空白も含めてそのまま保持する。
        #[test]
        fn decoded_unicode_text_is_preserved(
            chars in proptest::collection::vec(proptest::char::any(), 0..80),
            score in 0.0f32..=1.0,
        ) {
            let text: String = chars.into_iter().chain(std::iter::once('文')).collect();
            let block = recognized_text_block(RecognizeResult {
                text: text.clone(), mean_confidence: score,
            }, Some(score)).unwrap();
            proptest::prop_assert_eq!(block.text, text);
            proptest::prop_assert_eq!(block.confidence, score);
        }

        /// 有効なモデル信頼度を全域でそのまま保持する。
        #[test]
        fn valid_model_scores_are_preserved(score in 0.0f32..=1.0) {
            let block = recognized_text_block(RecognizeResult {
                text: "recognized text".into(), mean_confidence: score,
            }, Some(score)).unwrap();
            proptest::prop_assert_eq!(block.confidence, score);
        }
    }

    #[test]
    fn parse_mock_lines_accepts_mock_prefix() {
        let lines = parse_mock_lines_from_source_id("mock: A |B|  ");
        assert_eq!(lines, Some(vec!["A".to_string(), "B".to_string()]));
    }

    #[test]
    fn parse_mock_lines_rejects_non_mock_prefix() {
        assert!(parse_mock_lines_from_source_id("fixture:sample").is_none());
    }

    #[test]
    fn parse_mock_lines_accepts_request_field() {
        let request = OcrRequest::new("fixture:sample").with_mock_lines([" A ", "", "B"]);
        let lines = parse_mock_lines_from_request(&request);
        assert_eq!(lines, Some(vec!["A".to_string(), "B".to_string()]));
    }

    #[test]
    fn parse_lines_from_image_path_detects_diagnostic_bands_on_explicit_fallback() {
        let image_path = temp_ppm_path("ndlocr_image_path");
        write_test_ppm_with_band(&image_path, 20, 20, 5, 5, 15, 15);
        let request = OcrRequest::new("fixture:sample")
            .with_image_path(&image_path)
            .with_parseq_paths("missing-model.onnx", "missing-charset.yaml")
            .with_parseq_failure_policy(ParseqFailurePolicy::FallbackToBandLabel);

        let lines = parse_lines_from_image_path(&request).expect("image load must succeed");
        let ImagePage::Text(lines) = lines.expect("line bands should exist") else {
            panic!("expected text bands")
        };
        assert!(!lines.is_empty());
        assert!(lines[0].text.starts_with("band:"));

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn parse_lines_from_image_path_with_parseq_paths_reports_engine_failure() {
        let image_path = temp_ppm_path("ndlocr_image_path_parseq_error");
        write_test_ppm_with_band(&image_path, 20, 20, 5, 5, 15, 15);
        let request = OcrRequest::new("fixture:sample")
            .with_image_path(&image_path)
            .with_parseq_paths("missing-model.onnx", "missing-charset.yaml");

        let result = parse_lines_from_image_path(&request);
        assert!(
            matches!(result, Err(OcrError::EngineFailure(message)) if message.contains("PARSeq"))
        );

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn parse_lines_from_image_path_falls_back_to_band_label_when_policy_is_set() {
        let image_path = temp_ppm_path("ndlocr_image_path_parseq_fallback");
        write_test_ppm_with_band(&image_path, 20, 20, 5, 5, 15, 15);
        let request = OcrRequest::new("fixture:sample")
            .with_image_path(&image_path)
            .with_parseq_paths("missing-model.onnx", "missing-charset.yaml")
            .with_parseq_failure_policy(ParseqFailurePolicy::FallbackToBandLabel);

        let lines = parse_lines_from_image_path(&request).expect("fallback should succeed");
        let ImagePage::Text(lines) = lines.expect("lines should exist") else {
            panic!("expected text bands")
        };
        assert!(!lines.is_empty());
        assert!(lines[0].text.starts_with("band:"));

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn parse_lines_from_image_path_with_parseq_and_min_confidence_falls_back() {
        let image_path = temp_ppm_path("ndlocr_image_path_parseq_conf_fallback");
        write_test_ppm_with_band(&image_path, 20, 20, 5, 5, 15, 15);
        let request = OcrRequest::new("fixture:sample")
            .with_image_path(&image_path)
            .with_parseq_paths("missing-model.onnx", "missing-charset.yaml")
            .with_parseq_min_confidence(0.99)
            .with_parseq_failure_policy(ParseqFailurePolicy::FallbackToBandLabel);

        let lines = parse_lines_from_image_path(&request).expect("fallback should succeed");
        let ImagePage::Text(lines) = lines.expect("lines should exist") else {
            panic!("expected text bands")
        };
        assert!(!lines.is_empty());
        assert!(lines[0].text.starts_with("band:"));

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn apply_output_filters_respects_min_len_max_lines_and_symbol_ratio() {
        let request = OcrRequest::new("fixture:sample")
            .with_min_line_length(2)
            .with_max_symbol_ratio(0.5)
            .with_max_output_lines(2);
        let input = vec![
            "A".to_string(),
            "Hello".to_string(),
            "!!!".to_string(),
            "AB".to_string(),
            "World".to_string(),
        ];
        let filtered =
            apply_output_filters(input.into_iter().map(mock_text_block).collect(), &request)
                .expect("ordinary mock filters should succeed");
        assert_eq!(
            filtered
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["Hello", "AB"]
        );
    }

    #[test]
    fn symbol_ratio_counts_non_alphanumeric_chars() {
        let ratio = symbol_ratio("ab!?");
        assert!((ratio - 0.5).abs() < f32::EPSILON);
    }

    fn temp_ppm_path(prefix: &str) -> std::path::PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time error")
            .as_nanos();
        env::temp_dir().join(format!("{prefix}_{timestamp}.ppm"))
    }

    fn write_test_ppm_with_band(
        path: &std::path::Path,
        width: usize,
        height: usize,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
    ) {
        let mut body = format!("P3\n{width} {height}\n255\n");
        for y in 0..height {
            for x in 0..width {
                let in_band = x >= x0 && x < x1 && y >= y0 && y < y1;
                if in_band {
                    body.push_str("0 0 0\n");
                } else {
                    body.push_str("255 255 255\n");
                }
            }
        }
        fs::write(path, body).expect("failed to write test ppm");
    }
}
