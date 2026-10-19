//! native文字を残してOCRする領域の計画と、認識行の配置。
use super::*;

/// 一度の描画・認識に対応する計画。
pub(super) struct Target {
    /// 描画寸法・切り出し座標と残り本文容量。
    pub raster: PageOcrRaster,
    /// 認識後に除去する元画像と、本文の配置範囲。全ページ置換時は未指定。
    pub image: Option<(usize, jade_pdf2docx_parser::ParsedBounds)>,
}

/// native本文のあるページでは、他の描画と重ならない未認識画像だけを計画する。
/// 全領域分のページ描画容量をcallback前に検査する。
pub(super) fn plan(
    parsed: &ParsedDocument,
    pages: &BTreeSet<usize>,
    limits: Option<&PageOcrLimits>,
    allow_regions: bool,
) -> Result<Vec<Target>, ConvertError> {
    let rasters = limits
        .map(|limits| page_ocr_budget::plan(parsed, pages, limits))
        .transpose()?;
    let mut targets = Vec::new();
    let mut total = 0u64;
    for &page in pages {
        let raster = rasters
            .as_ref()
            .map(|rasters| rasters[&page])
            .unwrap_or(PageOcrRaster {
                text: jade_pdf2docx_ocr::OcrTextLimits::unlimited(),
                page,
                width_px: 0,
                height_px: 0,
                region: None,
            });
        let native = parsed
            .text_elements
            .iter()
            .any(|text| text.page == page && jade_pdf2docx_parser::text_is_visible(parsed, text));
        if !native {
            targets.push(Target {
                raster,
                image: None,
            });
            continue;
        }
        if !allow_regions {
            return Err(ConvertError::Ocr(format!(
                "page {}: native text requires region OCR",
                page + 1
            )));
        }
        let limits = limits.expect("region API always supplies limits");
        let geometry = parsed
            .pages
            .iter()
            .find(|geometry| geometry.page_number == page)
            .expect("raster geometry was validated");
        for (index, image) in parsed
            .image_elements
            .iter()
            .enumerate()
            .filter(|(_, image)| {
                image.page == page && jade_pdf2docx_parser::image_requires_ocr(parsed, image)
            })
        {
            let bounds = clipped(image.bounds, geometry);
            if parsed
                .image_elements
                .iter()
                .enumerate()
                .any(|(other_index, other)| {
                    other_index != index
                        && other.page == page
                        && overlaps(bounds, clipped(other.bounds, geometry))
                })
            {
                return Err(ConvertError::Ocr(format!(
                    "page {}: overlapping images require an unambiguous OCR region",
                    page + 1
                )));
            }
            let region = pixel_region(bounds, geometry, limits.dpi)?;
            targets.push(Target {
                raster: PageOcrRaster {
                    region: Some(region),
                    ..raster
                },
                image: Some((index, bounds)),
            });
        }
    }
    if let Some(limits) = limits {
        for target in &targets {
            let pixels =
                u64::from(target.raster.width_px + 1) * u64::from(target.raster.height_px + 1);
            total = total
                .checked_add(pixels)
                .ok_or_else(|| ConvertError::Ocr("OCR pixel budget overflow".into()))?;
            if total > limits.max_total_pixels {
                return Err(ConvertError::Ocr(
                    "OCR pixel budget exceeded for region renders".into(),
                ));
            }
        }
    }
    Ok(targets)
}

/// 画像の有限な範囲をページ矩形へ制限する。
fn clipped(
    bounds: jade_pdf2docx_parser::ParsedBounds,
    page: &ParsedPage,
) -> jade_pdf2docx_parser::ParsedBounds {
    jade_pdf2docx_parser::ParsedBounds {
        x0: bounds.x0.min(bounds.x1).max(0.0),
        x1: bounds.x0.max(bounds.x1).min(page.width),
        y0: bounds.y0.min(bounds.y1).max(0.0),
        y1: bounds.y0.max(bounds.y1).min(page.height),
    }
}

/// 正の面積を共有する画像の合成を曖昧なOCR対象として検出する。
fn overlaps(a: jade_pdf2docx_parser::ParsedBounds, b: jade_pdf2docx_parser::ParsedBounds) -> bool {
    a.x0.max(b.x0) < a.x1.min(b.x1) && a.y0.max(b.y0) < a.y1.min(b.y1)
}

/// PDFの左下原点から、回転後の画像の左上原点へ切り出し矩形を変換する。
pub(super) fn pixel_region(
    bounds: jade_pdf2docx_parser::ParsedBounds,
    page: &ParsedPage,
    dpi: u32,
) -> Result<[usize; 4], ConvertError> {
    let rotation = page.rotation.rem_euclid(360);
    let (x0, y0, x1, y1) = match rotation {
        0 => (
            bounds.x0,
            page.height - bounds.y1,
            bounds.x1,
            page.height - bounds.y0,
        ),
        90 => (bounds.y0, bounds.x0, bounds.y1, bounds.x1),
        180 => (
            page.width - bounds.x1,
            bounds.y0,
            page.width - bounds.x0,
            bounds.y1,
        ),
        270 => (
            page.height - bounds.y1,
            page.width - bounds.x1,
            page.height - bounds.y0,
            page.width - bounds.x0,
        ),
        _ => {
            return Err(ConvertError::Ocr(
                "unsupported OCR region page rotation".into(),
            ));
        }
    };
    let scale = f64::from(dpi) / 72.0;
    Ok([
        (f64::from(x0) * scale).floor() as usize,
        (f64::from(y0) * scale).floor() as usize,
        (f64::from(x1) * scale).ceil() as usize,
        (f64::from(y1) * scale).ceil() as usize,
    ])
}

/// 認識行を元画像の範囲へ均等配置する。細かい字形・段組みの再現は行わない。
pub(super) fn recognized_lines(
    bounds: jade_pdf2docx_parser::ParsedBounds,
    lines: Vec<String>,
) -> Vec<StructuredTextLine> {
    let step = ((bounds.y1 - bounds.y0) / lines.len().max(1) as f32).min(12.0);
    lines
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            structured_line(
                jade_pdf2docx_parser::ParsedBounds {
                    y1: bounds.y1 - step * index as f32,
                    ..bounds
                },
                text,
            )
        })
        .collect()
}

/// 解析後に追加する行を座標と本文から作り、認識文字列を再正規化しない。
pub(super) fn structured_line(
    bounds: jade_pdf2docx_parser::ParsedBounds,
    text: String,
) -> StructuredTextLine {
    StructuredTextLine {
        top_y: bounds.y1,
        left_x: bounds.x0,
        right_x: bounds.x1,
        alignment: ParagraphAlignment::Left,
        runs: vec![StructuredTextRun {
            text: text.clone(),
            style: StructuredTextStyle::default(),
        }],
        split_from_row: false,
        text,
    }
}

/// native行の解析後に認識行を挿入し、空白・行境界・数字だけの本文を保持する。
/// 認識領域が白紙でも、元のページ境界は残す。
pub(super) fn merge_lines(
    pages: &mut Vec<StructuredTextPage>,
    recognized: &BTreeMap<usize, Vec<StructuredTextLine>>,
) {
    for (&page, lines) in recognized {
        let index = match pages.binary_search_by_key(&page, |entry| entry.page) {
            Ok(index) => index,
            Err(index) => {
                pages.insert(
                    index,
                    StructuredTextPage {
                        page,
                        lines: Vec::new(),
                    },
                );
                index
            }
        };
        let native = &mut pages[index].lines;
        for line in lines {
            let index = native
                .iter()
                .position(|existing| existing.top_y < line.top_y)
                .unwrap_or(native.len());
            native.insert(index, line.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// nativeのページ番号しか残らない白紙領域でも、空ページ境界を保存する。
    #[test]
    fn verified_blank_regions_keep_empty_page_boundaries() {
        let parsed = ParsedDocument {
            quality: QualityReport::default(),
            path: PathBuf::new(),
            byte_len: 0,
            header: "%PDF-1.4".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 4 },
            page_count: 2,
            pages: (0..2)
                .map(|page_number| ParsedPage {
                    page_number,
                    width: 200.0,
                    height: 200.0,
                    rotation: 0,
                })
                .collect(),
            graphics: Vec::new(),
            text_elements: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        };
        let regions = BTreeMap::from([(0, Vec::new()), (1, Vec::new())]);
        let docx =
            build_structured_docx_with_page_ocr(&parsed, &None, &[], &BTreeMap::new(), &regions)
                .unwrap();
        let bytes = pack_docx_to_bytes(docx, &[]).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(!xml.contains("no extractable content"));
        assert_eq!(xml.matches("w:type=\"page\"").count(), 1);
    }

    /// 四方向の回転で切り出す画素が画像領域を覆い、ページ内に収まる。
    #[test]
    fn pixel_regions_follow_clockwise_page_rotation() {
        let bounds = jade_pdf2docx_parser::ParsedBounds {
            x0: 20.0,
            y0: 30.0,
            x1: 80.0,
            y1: 70.0,
        };
        for (rotation, expected) in [
            (0, [20, 30, 80, 70]),
            (90, [30, 20, 70, 80]),
            (180, [120, 30, 180, 70]),
            (270, [30, 120, 70, 180]),
        ] {
            let page = ParsedPage {
                page_number: 0,
                width: 200.0,
                height: 100.0,
                rotation,
            };
            assert_eq!(pixel_region(bounds, &page, 72).unwrap(), expected);
        }
    }
    proptest::proptest! {
        /// 有効な部分領域は回転・解像度によらず正の面積を持ち、描画寸法を超えない。
        #[test]
        fn regions_stay_inside_display_pixels(x0 in 0u32..199, y0 in 0u32..299, dx in 1u32..200, dy in 1u32..300, dpi in 1u32..601, rotation in 0i32..4) {
            let page = ParsedPage { page_number: 0, width: 200.0, height: 300.0, rotation: rotation * 90 };
            let bounds = jade_pdf2docx_parser::ParsedBounds { x0: x0 as f32, y0: y0 as f32, x1: (x0 + dx.min(200 - x0)) as f32, y1: (y0 + dy.min(300 - y0)) as f32 };
            let [left, top, right, bottom] = pixel_region(bounds, &page, dpi).unwrap();
            let (width, height) = if rotation % 2 == 0 { (200, 300) } else { (300, 200) };
            proptest::prop_assert!(left < right && top < bottom);
            proptest::prop_assert!(right <= (f64::from(width) * f64::from(dpi) / 72.0).ceil() as usize);
            proptest::prop_assert!(bottom <= (f64::from(height) * f64::from(dpi) / 72.0).ceil() as usize);
        }
    }
}
