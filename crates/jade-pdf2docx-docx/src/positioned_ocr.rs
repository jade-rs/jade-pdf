//! OCRの画素座標を検証し、native本文と同じ回転済み表示ページ座標へ戻す。
use super::{ConvertError, PageOcrRaster, ParsedPage, parsed_page_display_size_points};
use jade_pdf2docx_parser::ParsedBounds;

/// 行矩形が認識対象内にあることを確認し、DPIと上下原点だけを変換する。
pub(super) fn pdf_bounds(
    page: &ParsedPage,
    raster: PageOcrRaster,
    dpi: u32,
    bounds: [usize; 4],
) -> Result<ParsedBounds, ConvertError> {
    let [x0, y0, x1, y1] = bounds;
    let [left, top, right, bottom] = raster.region.unwrap_or([
        0,
        0,
        raster.width_px as usize + 1,
        raster.height_px as usize + 1,
    ]);
    if dpi == 0 || x0 >= x1 || y0 >= y1 || x0 < left || y0 < top || x1 > right || y1 > bottom {
        return Err(ConvertError::Ocr(format!(
            "page {}: invalid OCR line bounds",
            raster.page + 1
        )));
    }
    if !matches!(page.rotation.rem_euclid(360), 0 | 90 | 180 | 270) {
        return Err(ConvertError::Ocr(
            "unsupported OCR line page rotation".into(),
        ));
    }
    let (_, height) = parsed_page_display_size_points(page);
    let scale = 72.0 / f64::from(dpi);
    Ok(ParsedBounds {
        x0: (x0 as f64 * scale) as f32,
        y0: height - (y1 as f64 * scale) as f32,
        x1: (x1 as f64 * scale) as f32,
        y1: height - (y0 as f64 * scale) as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 四方向のページ回転で、画像上の行矩形をnativeと同じ表示位置へ戻す。
    #[test]
    fn line_positions_follow_page_rotation() {
        for (rotation, expected) in [
            (0, [10., 60., 30., 80.]),
            (90, [10., 160., 30., 180.]),
            (180, [10., 60., 30., 80.]),
            (270, [10., 160., 30., 180.]),
        ] {
            let page = ParsedPage {
                page_number: 0,
                width: 200.,
                height: 100.,
                rotation,
            };
            let raster = PageOcrRaster {
                page: 0,
                width_px: 200,
                height_px: 200,
                region: None,
                text: Default::default(),
            };
            let bounds = pdf_bounds(&page, raster, 72, [10, 20, 30, 40]).unwrap();
            assert_eq!([bounds.x0, bounds.y0, bounds.x1, bounds.y1], expected);
        }
    }

    proptest! {
        /// 任意DPI・回転の行矩形は、画素への再変換でも丸め1画素以内で保存する。
        #[test]
        fn line_bounds_roundtrip(
            rotation in 0i32..4, dpi in 1u32..601,
            x0 in 0usize..100, y0 in 0usize..100,
            width in 1usize..100, height in 1usize..100,
        ) {
            let page = ParsedPage { page_number: 0, width: 200. * 72. / dpi as f32, height: 200. * 72. / dpi as f32, rotation: rotation * 90 };
            let raster = PageOcrRaster { page: 0, width_px: 200, height_px: 200, region: Some([0, 0, 200, 200]), text: Default::default() };
            let original = [x0, y0, x0 + width, y0 + height];
            let pdf = pdf_bounds(&page, raster, dpi, original).unwrap();
            let returned = crate::region_ocr::pixel_region(pdf, &page, dpi).unwrap();
            for (actual, expected) in returned.into_iter().zip(original) {
                prop_assert!(actual.abs_diff(expected) <= 1);
            }
        }
    }
}
