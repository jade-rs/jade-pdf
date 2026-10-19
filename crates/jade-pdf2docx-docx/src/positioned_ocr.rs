//! OCRの画素座標を検証し、回転前のPDFページ座標へ戻す。
use super::{ConvertError, PageOcrRaster, ParsedPage};
use jade_pdf2docx_parser::ParsedBounds;

/// 行矩形が認識対象内にあることを確認し、DPIとページ回転を逆変換する。
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
    let scale = 72.0 / f64::from(dpi);
    let transform = |x: usize, y: usize| -> Result<(f32, f32), ConvertError> {
        let x = (x as f64 * scale) as f32;
        let y = (y as f64 * scale) as f32;
        Ok(match page.rotation.rem_euclid(360) {
            0 => (x, page.height - y),
            90 => (y, x),
            180 => (page.width - x, y),
            270 => (page.width - y, page.height - x),
            _ => {
                return Err(ConvertError::Ocr(
                    "unsupported OCR line page rotation".into(),
                ));
            }
        })
    };
    let (ax, ay) = transform(x0, y0)?;
    let (bx, by) = transform(x1, y1)?;
    Ok(ParsedBounds {
        x0: ax.min(bx),
        y0: ay.min(by),
        x1: ax.max(bx),
        y1: ay.max(by),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 四方向のページ回転で、画像上の行矩形を元のPDF位置へ戻す。
    #[test]
    fn line_positions_follow_page_rotation() {
        for (rotation, expected) in [
            (0, [10., 60., 30., 80.]),
            (90, [20., 10., 40., 30.]),
            (180, [170., 20., 190., 40.]),
            (270, [160., 70., 180., 90.]),
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
