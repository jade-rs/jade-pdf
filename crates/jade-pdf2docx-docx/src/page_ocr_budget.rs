//! OCRページの描画容量をcallback開始前に予約する。
use super::*;

/// OCR描画の解像度・画素上限と、文書全体の本文上限。
#[derive(Debug, Clone)]
pub struct PageOcrLimits {
    /// 全OCRページで共有する本文のbyte・行数上限。
    pub text: jade_pdf2docx_ocr::OcrTextLimits,
    /// 1インチあたりの描画画素数。
    pub dpi: u32,
    /// 1ページの描画領域の画素上限。
    pub max_page_pixels: u64,
    /// 必要な全OCRページの累積描画画素上限。
    pub max_total_pixels: u64,
}

impl Default for PageOcrLimits {
    /// 150 DPI・1ページ2500万画素・文書1億画素、本文8 MiB・32768行を既定とする。
    fn default() -> Self {
        Self {
            text: Default::default(),
            dpi: 150,
            max_page_pixels: 25_000_000,
            max_total_pixels: 100_000_000,
        }
    }
}

/// 予算検査済みページの描画寸法と本文残量。
/// rendererは各辺+1画素で切り抜き、寸法超過を検出する。
#[derive(Debug, Clone, Copy)]
pub struct PageOcrRaster {
    /// このページの認識に使用できる文書の残り本文容量。
    pub text: jade_pdf2docx_ocr::OcrTextLimits,
    /// 0始まりの元ページ番号。
    pub page: usize,
    /// 回転後のページ幅を切り上げた画素数。
    pub width_px: u32,
    /// 回転後のページ高さを切り上げた画素数。
    pub height_px: u32,
    /// 切り出す左上原点の画素範囲[x0,y0,x1,y1]。未指定ならページ全体。
    pub region: Option<[usize; 4]>,
}

/// すべての必要ページを先に検査し、途中まで描画してから累積上限を検出しない。
pub(super) fn plan(
    parsed: &ParsedDocument,
    pages: &BTreeSet<usize>,
    limits: &PageOcrLimits,
) -> Result<BTreeMap<usize, PageOcrRaster>, ConvertError> {
    if limits.dpi == 0 || limits.max_page_pixels == 0 || limits.max_total_pixels == 0 {
        return Err(ConvertError::Ocr("invalid OCR pixel budget".into()));
    }
    let mut total = 0u64;
    let mut result = BTreeMap::new();
    for &page in pages {
        let geometry = parsed
            .pages
            .iter()
            .find(|item| item.page_number == page)
            .ok_or_else(|| {
                ConvertError::Ocr(format!("page {}: missing OCR raster geometry", page + 1))
            })?;
        let (width, height) = parsed_page_display_size_points(geometry);
        let width_px = dimension(width, limits.dpi)?;
        let height_px = dimension(height, limits.dpi)?;
        let pixels = u64::from(width_px + 1) * u64::from(height_px + 1);
        total = total
            .checked_add(pixels)
            .ok_or_else(|| ConvertError::Ocr("OCR pixel budget overflow".into()))?;
        if pixels > limits.max_page_pixels || total > limits.max_total_pixels {
            return Err(ConvertError::Ocr(format!(
                "page {}: OCR pixel budget exceeded (page={pixels}, total={total})",
                page + 1
            )));
        }
        result.insert(
            page,
            PageOcrRaster {
                text: limits.text,
                page,
                width_px,
                height_px,
                region: None,
            },
        );
    }
    Ok(result)
}

/// 非有限値・負値・整数変換の飽和を拒否し、Popplerの正のintに1画素の余地を残す。
fn dimension(points: f32, dpi: u32) -> Result<u32, ConvertError> {
    let pixels = (f64::from(points) * f64::from(dpi) / 72.0).ceil();
    if !points.is_finite() || points <= 0.0 || pixels < 1.0 || pixels >= f64::from(i32::MAX) {
        return Err(ConvertError::Ocr(
            "invalid OCR raster dimension or pixel budget overflow".into(),
        ));
    }
    Ok(pixels as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 非有限値・非正値・整数境界は寸法として受理しない。
    #[test]
    fn invalid_dimensions_are_rejected() {
        for points in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -1.0,
            f32::MAX,
        ] {
            assert!(dimension(points, 150).is_err());
        }
        assert!(dimension(1.0, 0).is_err());
    }

    /// 寸法計算だけを検証できる最小解析結果を作る。
    fn document(pages: Vec<ParsedPage>) -> ParsedDocument {
        ParsedDocument {
            quality: QualityReport::default(),
            path: PathBuf::new(),
            byte_len: 0,
            header: "%PDF-1.4".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 4 },
            page_count: pages.len(),
            pages,
            graphics: Vec::new(),
            text_elements: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        }
    }

    /// 不明なページ寸法を描画へ渡さず、回転後の辺を使用する。
    #[test]
    fn missing_geometry_fails_and_rotation_swaps_dimensions() {
        let pages = BTreeSet::from([0]);
        let limits = PageOcrLimits {
            dpi: 72,
            ..Default::default()
        };
        assert!(plan(&document(Vec::new()), &pages, &limits).is_err());
        for rotation in [90, 270, -90] {
            let parsed = document(vec![ParsedPage {
                page_number: 0,
                width: 200.25,
                height: 100.25,
                rotation,
            }]);
            let raster = plan(&parsed, &pages, &limits).unwrap()[&0];
            assert_eq!((raster.width_px, raster.height_px), (101, 201));
        }
    }

    /// 緩い呼出元予算でも文書の合計画素数をラップさせない。
    #[test]
    fn cumulative_arithmetic_overflow_is_rejected() {
        let pages = (0..20).collect();
        let parsed = document(
            (0..20)
                .map(|page_number| ParsedPage {
                    page_number,
                    width: 1_000_000_000.0,
                    height: 1_000_000_000.0,
                    rotation: 0,
                })
                .collect(),
        );
        let limits = PageOcrLimits {
            dpi: 72,
            max_page_pixels: u64::MAX,
            max_total_pixels: u64::MAX,
            ..Default::default()
        };
        let error = plan(&parsed, &pages, &limits).unwrap_err();
        assert!(error.to_string().contains("overflow"));
    }

    proptest! {
        /// 有効な寸法は切り上げ前の値以上で、誤差は1画素未満に収まる。
        #[test]
        fn valid_dimension_rounds_up(points in 0.01f32..100_000.0, dpi in 1u32..601) {
            let actual = f64::from(dimension(points, dpi).unwrap());
            let expected = f64::from(points) * f64::from(dpi) / 72.0;
            prop_assert!(actual >= expected);
            prop_assert!(actual - expected < 1.0);
        }
    }
}
