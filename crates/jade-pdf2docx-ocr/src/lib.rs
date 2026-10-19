//! 画像 PDF 向け OCR と、表を [`OcrTable`] に載せるためのユーティリティを提供する。
//!
//! エントリは [`run_ocr`]。`ndlocr-lite` feature で `ndlocr-lite-rs` 連携を有効化し、
//! `onnx` feature を明示すると CPU の ONNX Runtime で実画像を推論する。
//! `table-ppstructure` feature で幾何 JSON（`parse_geometry_table_json`）、外部表パイプライン、
//! 罫線からの格子推定（`cells_from_axis_aligned_line_segments`）などを利用できる。

mod correction;
mod engine;
mod engines;
mod hungarian;
mod page_content;
#[cfg(feature = "onnx")]
mod page_session;
#[cfg(feature = "onnx")]
mod page_timings;
mod path;
mod result;
mod table;
#[cfg(feature = "table-ppstructure")]
mod table_geometry_json;
#[cfg(feature = "table-ppstructure")]
mod table_lines_grid;
mod table_match;
#[cfg(feature = "table-ppstructure")]
mod table_pipeline;
mod text_budget;
mod types;

#[cfg(feature = "onnx")]
pub use page_session::PageOcrSession;
#[cfg(feature = "onnx")]
pub use page_timings::OcrPageTimings;

pub use text_budget::{OcrTextBudget, OcrTextLimits};

pub use engine::OcrEngine;
pub use engines::StubOcrEngine;
pub use hungarian::min_cost_assignment_square;
pub use jade_pdf2docx_ocr_correction as text_correction;
pub use page_content::{OcrPageContent, PositionedOcrLine, VerifiedBlankPage};
pub use path::{
    NDL_OCR_LITE_RS_GITHUB_URL, NDL_OCR_LITE_RS_RELATIVE_PATH_FROM_REPO_ROOT,
    resolve_ndlocr_workspace_path,
};
pub use result::{OcrResult, OcrTextBlock, OcrTextKind};
pub use table::{OcrCellSpan, OcrTable, OcrTableError};
#[cfg(feature = "table-ppstructure")]
pub use table_geometry_json::{GeometryTableJsonError, parse_geometry_table_json};
#[cfg(feature = "table-ppstructure")]
pub use table_lines_grid::cells_from_axis_aligned_line_segments;
pub use table_match::{
    AxisRect, TableMatchError, TextBox, cell_indices_reading_order,
    ocr_table_from_row_major_cells_and_texts, ocr_table_from_row_major_cells_and_texts_hungarian,
    ocr_table_from_text_boxes_projection, ocr_table_from_unsorted_cells_and_texts,
};
#[cfg(feature = "table-ppstructure")]
pub use table_pipeline::{
    TablePipelineError, TablePipelineOutput, parse_table_pipeline_json, run_table_pipeline_command,
};
pub use types::{OcrError, OcrRequest, OcrTextDirection, ParseqFailurePolicy};

#[cfg(feature = "ndlocr-lite")]
pub use engines::NdlOcrLiteEngine;

#[cfg(not(feature = "ndlocr-lite"))]
fn recognize_with_active_engine(request: &OcrRequest) -> Result<OcrResult, OcrError> {
    StubOcrEngine.recognize(request)
}

#[cfg(feature = "ndlocr-lite")]
fn recognize_with_active_engine(request: &OcrRequest) -> Result<OcrResult, OcrError> {
    NdlOcrLiteEngine.recognize(request)
}

/// 現在のデフォルト OCR 実行関数。
pub fn run_ocr(request: &OcrRequest) -> Result<OcrResult, OcrError> {
    recognize_with_active_engine(request)
}

/// ページの認識本文または全白の証明を返す。通常の空の認識結果は拒否する。
pub fn run_page_ocr(request: &OcrRequest) -> Result<OcrPageContent, OcrError> {
    #[cfg(feature = "ndlocr-lite")]
    {
        NdlOcrLiteEngine.recognize_page(request)
    }
    #[cfg(not(feature = "ndlocr-lite"))]
    {
        run_ocr(request)?
            .into_text_lines()
            .map(OcrPageContent::Text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "ndlocr-lite"))]
    #[test]
    fn run_ocr_uses_stub_engine_by_default() {
        let request = OcrRequest::new("fixture:sample");
        let result = run_ocr(&request).expect("stub OCR should succeed");
        assert_eq!(result.source_id, "fixture:sample");
        assert!(result.blocks.is_empty());
    }

    #[cfg(feature = "ndlocr-lite")]
    #[test]
    fn run_ocr_uses_ndlocr_bridge_for_request_mock_lines() {
        let request = OcrRequest::new("fixture:sample").with_mock_lines(["line1", "line2"]);
        let result = run_ocr(&request).expect("ndlocrlite bridge should accept mock source");
        assert_eq!(result.blocks.len(), 2);
        assert_eq!(result.blocks[0].text, "line1");
        assert_eq!(result.blocks[1].text, "line2");
        assert!(
            result
                .blocks
                .iter()
                .all(|block| block.kind == OcrTextKind::Mock)
        );
    }

    #[cfg(feature = "ndlocr-lite")]
    #[test]
    /// モデル未指定の画像を公開APIからも拒否する。
    fn run_ocr_rejects_image_without_parseq_paths() {
        use std::{
            env, fs,
            time::{SystemTime, UNIX_EPOCH},
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time error")
            .as_nanos();
        let image_path = env::temp_dir().join(format!("jade_pdf2docx_ocr_img_{timestamp}.ppm"));
        let mut body = String::from("P3\n20 20\n255\n");
        for y in 0..20 {
            for x in 0..20 {
                let in_band = (5..15).contains(&x) && (5..15).contains(&y);
                if in_band {
                    body.push_str("0 0 0\n");
                } else {
                    body.push_str("255 255 255\n");
                }
            }
        }
        fs::write(&image_path, body).expect("failed to write test image");

        let request = OcrRequest::new("fixture:image").with_image_path(&image_path);
        let result = run_ocr(&request);
        assert!(
            matches!(result, Err(OcrError::InvalidRequest(message)) if message.contains("PARSeq"))
        );

        let _ = fs::remove_file(image_path);
    }

    #[cfg(feature = "ndlocr-lite")]
    #[test]
    fn run_ocr_rejects_non_mock_source_on_ndlocr_bridge() {
        let request = OcrRequest::new("fixture:sample");
        let result = run_ocr(&request);
        assert!(
            matches!(result, Err(OcrError::EngineFailure(message)) if message.contains("with_image_path"))
        );
    }

    #[cfg(feature = "ndlocr-lite")]
    #[test]
    fn run_ocr_falls_back_to_band_label_when_parseq_policy_allows() {
        use std::{
            env, fs,
            time::{SystemTime, UNIX_EPOCH},
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time error")
            .as_nanos();
        let image_path = env::temp_dir().join(format!(
            "jade_pdf2docx_ocr_img_parseq_fallback_{timestamp}.ppm"
        ));
        let mut body = String::from("P3\n20 20\n255\n");
        for y in 0..20 {
            for x in 0..20 {
                let in_band = (5..15).contains(&x) && (5..15).contains(&y);
                if in_band {
                    body.push_str("0 0 0\n");
                } else {
                    body.push_str("255 255 255\n");
                }
            }
        }
        fs::write(&image_path, body).expect("failed to write test image");

        let request = OcrRequest::new("fixture:image")
            .with_image_path(&image_path)
            .with_parseq_paths("missing-model.onnx", "missing-charset.yaml")
            .with_parseq_failure_policy(ParseqFailurePolicy::FallbackToBandLabel);
        let result = run_ocr(&request).expect("fallback should avoid parseq hard failure");
        assert!(!result.blocks.is_empty());
        assert!(result.blocks[0].text.starts_with("band:"));
        assert_eq!(result.blocks[0].kind, OcrTextKind::BandDiagnostic);
        assert_eq!(result.blocks[0].confidence, 0.0);
        assert!(result.into_text_lines().is_err());

        let _ = fs::remove_file(image_path);
    }
}
