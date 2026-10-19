//! OCRが必要なページだけを認識本文へ置換するファイル変換。
use super::*;
use jade_pdf2docx_parser::QualityIssue;

/// OCRが必要な画像ページだけを補い、nativeページの本文・表・画像を保持する。
/// callbackには0始まりのページ番号を渡す。行の空白と順序を変更しない。
/// 呼出元は同じPDFを認識し、callback内の資源・時間上限を適用すること。
/// strictではOCR以外の品質欠落を先に拒否する。失敗時は既存出力を保持する。
/// 全体置換用の`ocr_text_lines`と`ocr_table`の併用は拒否する。
/// 同じページにnative文字がある場合は領域OCR用APIを使うこと。ページ全体置換は拒否する。
pub fn convert_minimum_pdf_to_docx_with_page_ocr(
    request: &MinimumPdfToDocxRequest,
    options: &ParseOptions,
    mut recognize: impl FnMut(usize) -> Result<Vec<String>, String>,
) -> Result<MinimumConversionReport, ConvertError> {
    convert(request, options, None, false, false, |raster| {
        recognize(raster.page).map(OcrPageContent::Text)
    })
}

/// 全OCRページの描画容量を検査し、本文容量を累積検査する。
/// callbackは検査済み寸法を描画上限として使い、超過したPNGを本文認識へ渡さないこと。
/// callbackは残り本文容量も認識エンジンへ渡すこと。返却行を保持する前にも再検査する。
/// 画素上限はnative codec・モデル・DOCXを含むプロセス総RSSの上限ではない。
pub fn convert_minimum_pdf_to_docx_with_bounded_page_ocr(
    request: &MinimumPdfToDocxRequest,
    options: &ParseOptions,
    limits: &PageOcrLimits,
    mut recognize: impl FnMut(PageOcrRaster) -> Result<Vec<String>, String>,
) -> Result<MinimumConversionReport, ConvertError> {
    convert(request, options, Some(limits), false, false, |raster| {
        recognize(raster).map(OcrPageContent::Text)
    })
}

/// 画素・本文上限を適用し、検証済み白紙を本文容量を消費せず保持する。
/// callbackは同じPDFの該当ページ全体を描画して白紙を証明すること。
/// 描画寸法と認識本文の残量は従来の制限付きAPIと同様に適用すること。
/// 空の認識本文は白紙として扱わず、失敗時は既存出力を保持する。
pub fn convert_minimum_pdf_to_docx_with_bounded_page_ocr_content(
    request: &MinimumPdfToDocxRequest,
    options: &ParseOptions,
    limits: &PageOcrLimits,
    recognize: impl FnMut(PageOcrRaster) -> Result<OcrPageContent, String>,
) -> Result<MinimumConversionReport, ConvertError> {
    convert(request, options, Some(limits), true, false, recognize)
}

/// 画像領域を認識して、元のnative本文と組み合わせる。
/// callbackはページ全体を予算内で描画し、`region`があればその領域だけを認識すること。
/// 白紙の証明も同じ領域に適用する。重なる画像は曖昧なため認識前に拒否する。
/// 画像に重なるnative文字は座標付きOCRとの一致を検査し、曖昧な重複を拒否する。
/// 領域ごとのページ描画容量と文書全体の本文容量を検査し、失敗時は既存出力を保持する。
pub fn convert_minimum_pdf_to_docx_with_bounded_region_ocr_content(
    request: &MinimumPdfToDocxRequest,
    options: &ParseOptions,
    limits: &PageOcrLimits,
    recognize: impl FnMut(PageOcrRaster) -> Result<OcrPageContent, String>,
) -> Result<MinimumConversionReport, ConvertError> {
    convert(request, options, Some(limits), true, true, recognize)
}

/// 解析と全ページの検査を終えてから認識し、成功時だけ出力を公開する。
fn convert(
    request: &MinimumPdfToDocxRequest,
    options: &ParseOptions,
    limits: Option<&PageOcrLimits>,
    allow_blank: bool,
    allow_regions: bool,
    mut recognize: impl FnMut(PageOcrRaster) -> Result<OcrPageContent, String>,
) -> Result<MinimumConversionReport, ConvertError> {
    conversion_control::check(options)?;
    output::validate_distinct_paths(&request.input_pdf_path, &request.output_docx_path)?;
    if request.ocr_text_lines.is_some() || request.ocr_table.is_some() {
        return Err(ConvertError::Ocr(
            "page OCR cannot be combined with global OCR content".into(),
        ));
    }
    let mut parsing = options.clone();
    parsing.strict = false;
    let mut parsed = parse_pdf_with_options(&request.input_pdf_path, &parsing)?;
    let pages: BTreeSet<_> = parsed
        .quality
        .issues
        .iter()
        .filter_map(|issue| match issue {
            QualityIssue::OcrRequired { page } => Some(*page),
            _ => None,
        })
        .collect();
    let unresolved = QualityReport {
        issues: parsed
            .quality
            .issues
            .iter()
            .filter(|issue| !matches!(issue, QualityIssue::OcrRequired { .. }))
            .cloned()
            .collect(),
    };
    if options.strict && !unresolved.issues.is_empty() {
        return Err(ParseError::QualityRejected {
            path: parsed.path,
            report: unresolved,
        }
        .into());
    }
    let targets = region_ocr::plan(&parsed, &pages, limits, allow_regions)?;
    let mut removed_images = BTreeSet::new();
    let mut recognized = BTreeMap::new();
    let mut region_lines: BTreeMap<usize, Vec<StructuredTextLine>> = BTreeMap::new();
    let mut text_budget = jade_pdf2docx_ocr::OcrTextBudget::new(
        limits.map_or(jade_pdf2docx_ocr::OcrTextLimits::unlimited(), |limits| {
            limits.text
        }),
    );
    for target in targets {
        let page = target.raster.page;
        conversion_control::check(options)?;
        if !allow_blank {
            text_budget
                .check_line_bytes(1)
                .map_err(|error| ConvertError::Ocr(format!("page {}: {error}", page + 1)))?;
        }
        let mut raster = target.raster;
        raster.text = text_budget.remaining();
        let content = recognize(raster)
            .map_err(|error| ConvertError::Ocr(format!("page {}: {error}", page + 1)))?;
        conversion_control::check(options)?;
        let blank = matches!(&content, OcrPageContent::Blank(_));
        let mut positions = None;
        let lines = match content {
            OcrPageContent::Blank(_) => Vec::new(),
            OcrPageContent::Text(lines) => lines,
            OcrPageContent::PositionedText(lines) => {
                let (text, bounds): (Vec<_>, Vec<_>) = lines
                    .into_iter()
                    .map(|line| (line.text, line.bounds))
                    .unzip();
                positions = Some(bounds);
                text
            }
        };
        if (!blank && lines.is_empty()) || lines.iter().any(|line| line.trim().is_empty()) {
            return Err(ConvertError::Ocr(format!(
                "page {}: empty recognized text",
                page + 1
            )));
        }
        for line in &lines {
            text_budget
                .reserve_line(line)
                .map_err(|error| ConvertError::Ocr(format!("page {}: {error}", page + 1)))?;
        }
        let positions = positions
            .map(|positions| {
                let limits = limits.ok_or_else(|| {
                    ConvertError::Ocr("positioned OCR requires bounded raster dimensions".into())
                })?;
                let geometry = parsed
                    .pages
                    .iter()
                    .find(|geometry| geometry.page_number == page)
                    .expect("raster geometry was validated");
                positions
                    .into_iter()
                    .map(|bounds| positioned_ocr::pdf_bounds(geometry, raster, limits.dpi, bounds))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        if let Some((index, bounds)) = target.image {
            let lines = ocr_reconcile::reconcile(&parsed, page, bounds, lines, positions)?;
            region_lines.entry(page).or_default().extend(lines);
            removed_images.insert(index);
        } else {
            recognized.insert(page, lines);
        }
    }
    let mut index = 0;
    parsed.image_elements.retain(|_| {
        let keep = !removed_images.contains(&index);
        index += 1;
        keep
    });
    parsed.quality = unresolved;
    let tables = infer_positioned_tables_from_pdf_graphics(&parsed);
    conversion_control::check(options)?;
    let mut docx =
        build_structured_docx_with_page_ocr(&parsed, &None, &tables, &recognized, &region_lines)?;
    if !request.clean_output {
        docx = docx.add_paragraph(
            Paragraph::new().add_run(text_run(format!("source: {}", parsed.path.display()))),
        );
    }
    conversion_control::check(options)?;
    let version = format!("{}.{}", parsed.version.major, parsed.version.minor);
    let mut preview = Vec::new();
    let mut preview_pages = text_lines_by_page_from_pdf_text_elements(&parsed, &tables);
    region_ocr::merge_lines(&mut preview_pages, &region_lines);
    for page in preview_pages {
        if let Some(lines) = recognized.get(&page.page) {
            preview.extend(lines.iter().cloned());
        } else {
            preview.extend(page.lines.into_iter().map(|line| line.text));
        }
    }
    let preview = preview.join("\n");
    let bytes = pack_docx_to_bytes(docx, &parsed.embedded_fonts)?;
    conversion_control::check(options)?;
    output::write_atomic(&request.output_docx_path, &bytes, &mut |_| {
        conversion_control::check(options)
    })?;
    Ok(MinimumConversionReport {
        quality: parsed.quality,
        source_pdf_path: parsed.path,
        source_pdf_byte_len: parsed.byte_len,
        source_pdf_header: parsed.header,
        source_pdf_version: version,
        source_text_preview: preview,
        output_docx_path: request.output_docx_path.clone(),
    })
}

/// 認識済み行を再正規化せず、1行1段落として追加する。
pub(super) fn add_lines<C: StructuredPageContainer>(mut container: C, lines: &[String]) -> C {
    for line in lines {
        container = container.add_structured_paragraph(Paragraph::new().add_run(text_run(line)));
    }
    container
}
