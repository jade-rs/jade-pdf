//! 認識本文と診断を区別し、要求された OCR の失敗を保存前に通知する。

use std::path::{Path, PathBuf};

use jade_pdf2docx_ocr::{OcrRequest, run_ocr};
use jade_pdf2docx_pdf_raster::{
    pdftoppm_available, render_pdf_pages_to_png, render_selected_pdf_pages_to_png,
};

use crate::cli::{CliArgs, ParseqFailurePolicyArg};

/// CLI で明示されたモデル、信頼度、失敗ポリシーを適用する。
fn apply_parseq_to_request(mut request: OcrRequest, parsed: &CliArgs) -> OcrRequest {
    let policy = parsed
        .ocr_parseq_failure_policy
        .unwrap_or(ParseqFailurePolicyArg::FailFast);
    request = request.with_parseq_failure_policy(match policy {
        ParseqFailurePolicyArg::FailFast => jade_pdf2docx_ocr::ParseqFailurePolicy::FailFast,
        ParseqFailurePolicyArg::FallbackToBandLabel => {
            jade_pdf2docx_ocr::ParseqFailurePolicy::FallbackToBandLabel
        }
    });
    if let (Some(model), Some(charset)) = (
        parsed.ocr_parseq_model.as_deref(),
        parsed.ocr_charset.as_deref(),
    ) {
        request = request.with_parseq_paths(PathBuf::from(model), PathBuf::from(charset));
    }
    if let Some(min_confidence) = parsed.ocr_min_confidence {
        request = request.with_parseq_min_confidence(min_confidence);
    }
    request
}

/// ラスタ化して本文 OCR を実行するページの指定。
enum PdfOcrPageSelection {
    /// 全ページを認識する。
    All,
    /// 指定されたゼロ始まりのページを認識する。
    Pages(Vec<usize>),
}

/// 全対象ページの認識に成功した場合だけ、結合した本文を返す。
fn collect_ocr_from_pdf_pages(
    parsed: &CliArgs,
    input_pdf: &Path,
    selection: PdfOcrPageSelection,
) -> Result<Vec<String>, String> {
    if parsed.ocr_parseq_model.is_none() || parsed.ocr_charset.is_none() {
        return Err("PDF text OCR requires PARSeq model and charset paths".into());
    }
    if !pdftoppm_available() {
        return Err("pdftoppm not found; install poppler for PDF text OCR".into());
    }
    let tmp = tempfile::Builder::new()
        .prefix("jade_pdf2docx_ocr_pdf_")
        .tempdir()
        .map_err(|e| format!("failed to create OCR temp directory: {e}"))?;
    let pages = match selection {
        PdfOcrPageSelection::All => {
            render_pdf_pages_to_png(input_pdf, tmp.path(), parsed.ocr_pdf_dpi)
        }
        PdfOcrPageSelection::Pages(ref pages) => {
            render_selected_pdf_pages_to_png(input_pdf, tmp.path(), parsed.ocr_pdf_dpi, pages)
        }
    }
    .map_err(|e| format!("PDF raster failed: {e}"))?;
    if pages.is_empty() {
        return Err("PDF produced no raster pages for OCR".into());
    }
    let mut merged = Vec::new();
    for png in pages {
        let request = apply_parseq_to_request(
            OcrRequest::new("example:minimum-docx:pdf-page").with_image_path(png.clone()),
            parsed,
        );
        let lines = run_ocr(&request)
            .and_then(|result| result.into_text_lines())
            .map_err(|e| format!("OCR page {}: {e}", png.display()))?;
        if !merged.is_empty() {
            merged.push(String::new());
        }
        merged.extend(lines);
    }
    Ok(merged)
}

/// 優先順位に従って本文を取得し、全認識源の失敗を成功として扱わない。
pub(crate) fn collect_optional_ocr_lines(
    parsed: &CliArgs,
    input_pdf: &Path,
) -> Result<Option<Vec<String>>, String> {
    let mut failures = Vec::new();
    // モデル未指定の外部表パイプラインでは画像指定を表専用入力として扱う。
    let table_only = parsed.ocr_table_pipeline.is_some() && parsed.ocr_parseq_model.is_none();
    if !table_only {
        if let Some(image_path) = parsed.ocr_image.as_deref() {
            let request = apply_parseq_to_request(
                OcrRequest::new("example:minimum-docx").with_image_path(PathBuf::from(image_path)),
                parsed,
            );
            match run_ocr(&request).and_then(|result| result.into_text_lines()) {
                Ok(lines) => return Ok(Some(lines)),
                Err(e) => failures.push(format!("image OCR: {e}")),
            }
        }
        match resolve_pdf_ocr_page_selection(parsed, input_pdf) {
            Ok(Some(selection)) => match collect_ocr_from_pdf_pages(parsed, input_pdf, selection) {
                Ok(lines) => {
                    warn_about_fallback(&failures, "PDF OCR");
                    return Ok(Some(lines));
                }
                Err(e) => failures.push(e),
            },
            Ok(None) => {}
            Err(e) => failures.push(e),
        }
    }
    if let Some(mock) = parsed.ocr_mock.as_deref() {
        let lines = mock
            .split('|')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let result = run_ocr(&OcrRequest::new("example:minimum-docx").with_mock_lines(lines))
            .and_then(|result| result.into_text_lines())
            .map_err(|e| format!("explicit mock OCR: {e}"))?;
        warn_about_fallback(&failures, "explicit --ocr-mock");
        return Ok(Some(result));
    }
    if failures.is_empty() {
        Ok(None)
    } else {
        Err(failures.join("; "))
    }
}

/// 先行する認識源の失敗と実際に採用する入力を通知する。
fn warn_about_fallback(failures: &[String], source: &str) {
    if !failures.is_empty() {
        eprintln!("warning: {}; using {source}", failures.join("; "));
    }
}

/// 自動判定または明示指定から PDF OCR の対象ページを決める。
fn resolve_pdf_ocr_page_selection(
    parsed: &CliArgs,
    input_pdf: &Path,
) -> Result<Option<PdfOcrPageSelection>, String> {
    if parsed.ocr_from_pdf {
        return Ok(Some(PdfOcrPageSelection::All));
    }
    if !parsed.ocr_auto {
        return Ok(None);
    }
    let document = jade_pdf2docx_parser::parse_pdf(input_pdf)
        .map_err(|e| format!("--ocr-auto could not inspect PDF before OCR: {e}"))?;
    let pages = jade_pdf2docx_docx::automatic_ocr_pdf_page_indices(&document);
    if pages.is_empty() {
        return Ok(None);
    }
    let display_pages = pages
        .iter()
        .map(|page| (page + 1).to_string())
        .collect::<Vec<_>>()
        .join(",");
    eprintln!("info: --ocr-auto enabled PDF raster OCR for pages: {display_pages}");
    Ok(Some(PdfOcrPageSelection::Pages(pages)))
}
