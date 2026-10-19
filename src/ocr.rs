//! 監督worker内で入力を固定し、必要なページだけを実OCRする。
use crate::cli::Request;
use jade_pdf2docx_docx::{
    ConvertError, MinimumConversionReport, MinimumPdfToDocxRequest, ParseOptions,
};

/// OCR runtimeなしのbuildでは明示OCRを失敗として返す。
#[cfg(not(feature = "ocr-onnx"))]
pub(crate) fn convert(
    _request: &Request,
    _conversion: &MinimumPdfToDocxRequest,
    _options: &ParseOptions,
) -> Result<MinimumConversionReport, ConvertError> {
    Err(ConvertError::Ocr(
        "--ocr requires a build with the ocr-onnx feature".into(),
    ))
}

/// 同一snapshotを解析と描画へ渡し、OCRを監督対象のworker内で実行する。
#[cfg(feature = "ocr-onnx")]
pub(crate) fn convert(
    request: &Request,
    conversion: &MinimumPdfToDocxRequest,
    options: &ParseOptions,
) -> Result<MinimumConversionReport, ConvertError> {
    let ocr = request.ocr.as_ref().expect("OCR request was validated");
    // 親が所有するTMPDIR配下なので、workerの強制終了時も監督が回収する。
    let directory = tempfile::Builder::new()
        .prefix("jade-pdf2docx-ocr-")
        .tempdir()?;
    let input = directory.path().join("input.pdf");
    snapshot(
        &conversion.input_pdf_path,
        &input,
        options.limits.max_input_bytes,
    )?;
    let mut conversion = conversion.clone();
    conversion.input_pdf_path = input.clone();
    let limits = jade_pdf2docx_docx::PageOcrLimits {
        dpi: ocr.dpi,
        ..Default::default()
    };
    let mut session = jade_pdf2docx_ocr::PageOcrSession::default();
    let mut report =
        jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
            &conversion,
            options,
            &limits,
            |raster| {
                // ページごとに画像を回収し、ページ数に応じてPNGを溜めない。
                let rendered = tempfile::Builder::new()
                    .prefix("page-")
                    .tempdir_in(directory.path())
                    .map_err(|error| error.to_string())?;
                let image = jade_pdf2docx_pdf_raster::render_pdf_page_to_png_bounded(
                    &input,
                    rendered.path(),
                    ocr.dpi,
                    raster.page,
                    raster.width_px,
                    raster.height_px,
                    limits.max_page_pixels,
                )
                .map_err(|error| error.to_string())?;
                let mut inference =
                    jade_pdf2docx_ocr::OcrRequest::new(format!("page-{}", raster.page + 1))
                        .with_image_path(&image)
                        .with_parseq_paths(&ocr.model, &ocr.charset)
                        .with_text_direction(if ocr.vertical {
                            jade_pdf2docx_ocr::OcrTextDirection::Vertical
                        } else {
                            jade_pdf2docx_ocr::OcrTextDirection::Horizontal
                        })
                        .with_paper_shading(ocr.paper_shading)
                        .with_text_limits(raster.text)
                        .with_parseq_min_confidence(0.5);
                if let Some(region) = raster.region {
                    inference = inference.with_image_region(region);
                }
                session
                    .recognize_page(&inference)
                    .map_err(|error| error.to_string())
            },
        )?;
    report.source_pdf_path = request.input.clone();
    Ok(report)
}

/// 最大入力長+1byteだけ読み、解析と描画が共有する入力を専用領域へ固定する。
#[cfg(feature = "ocr-onnx")]
fn snapshot(
    source: &std::path::Path,
    destination: &std::path::Path,
    limit: u64,
) -> std::io::Result<()> {
    use std::{
        fs::{File, OpenOptions},
        io::{self, Read},
    };
    let source = File::open(source)?;
    if !source.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "OCR input must be a regular file",
        ));
    }
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let count = io::copy(&mut source.take(limit.saturating_add(1)), &mut destination)?;
    if count > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "OCR input bytes exceed parser limit",
        ));
    }
    Ok(())
}

#[cfg(all(test, feature = "ocr-onnx"))]
mod tests {
    use super::*;
    /// inclusive境界を保持し、超過コピーはlimit+1で止める。
    #[test]
    fn snapshot_bounds_and_original_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.pdf");
        std::fs::write(&source, b"12345").unwrap();
        let exact = directory.path().join("exact.pdf");
        snapshot(&source, &exact, 5).unwrap();
        assert_eq!(std::fs::read(exact).unwrap(), b"12345");
        let over = directory.path().join("over.pdf");
        assert!(snapshot(&source, &over, 3).is_err());
        assert_eq!(std::fs::read(over).unwrap(), b"1234");
        assert_eq!(std::fs::read(source).unwrap(), b"12345");
    }
}
