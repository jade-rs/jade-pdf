//! 全ページOCRの文字順と、ページ基準の行矩形を独立して保持する。
use super::*;

/// 本文を再整列せず、対応する任意の行frameを同じ添字で保持する。
pub(super) struct Lines {
    /// 認識器が返した空白・行順を維持する本文。
    pub(super) text: Vec<String>,
    /// 座標が明示された場合だけ持つ、ページ基準の矩形。
    frames: Option<Vec<Frame>>,
}

/// Wordのpage基準で表現する、検証済みのtwips座標。
struct Frame {
    /// 左端の絶対位置。
    x: i32,
    /// 上端の絶対位置。
    y: i32,
    /// 行矩形の幅。
    width: u32,
    /// 行矩形の高さ。
    height: u32,
}

impl Lines {
    /// 座標なしcallbackの従来の段落出力を保持する。
    pub(super) fn unpositioned(text: Vec<String>) -> Self {
        Self { text, frames: None }
    }

    /// PDF座標を表示座標へ変換し、文字列の順番は変更しない。
    pub(super) fn positioned(
        text: Vec<String>,
        bounds: Vec<ParsedBounds>,
        page: &ParsedPage,
    ) -> Result<Self, ConvertError> {
        if text.len() != bounds.len() {
            return Err(ConvertError::Ocr(
                "whole-page OCR text/position count changed".into(),
            ));
        }
        let frames = bounds
            .into_iter()
            .map(|bounds| Frame::from_pdf(bounds, page))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            text,
            frames: Some(frames),
        })
    }

    /// 可読本文の順序を変えず、座標付き行だけ独立したpage frameへ出力する。
    pub(super) fn add_to<C: StructuredPageContainer>(&self, mut container: C) -> C {
        for (index, line) in self.text.iter().enumerate() {
            let mut paragraph = Paragraph::new().add_run(text_run(line));
            if let Some(frames) = &self.frames {
                let frame = &frames[index];
                paragraph = paragraph
                    .h_anchor("page")
                    .v_anchor("page")
                    .wrap("none")
                    .h_rule("atLeast")
                    .frame_x(frame.x)
                    .frame_y(frame.y)
                    .frame_width(frame.width)
                    .frame_height(frame.height)
                    .line_spacing(LineSpacing::new().before(0).after(0));
            }
            container = container.add_structured_paragraph(paragraph);
        }
        container
    }
}

impl Frame {
    /// 画像の表示方向へ戻し、frameで表現不能な値を丸め込みで受け入れない。
    fn from_pdf(bounds: ParsedBounds, page: &ParsedPage) -> Result<Self, ConvertError> {
        let (x, y, width, height) = match page.rotation.rem_euclid(360) {
            0 => (
                bounds.x0,
                page.height - bounds.y1,
                bounds.x1 - bounds.x0,
                bounds.y1 - bounds.y0,
            ),
            90 => (
                bounds.y0,
                bounds.x0,
                bounds.y1 - bounds.y0,
                bounds.x1 - bounds.x0,
            ),
            180 => (
                page.width - bounds.x1,
                bounds.y0,
                bounds.x1 - bounds.x0,
                bounds.y1 - bounds.y0,
            ),
            270 => (
                page.height - bounds.y1,
                page.width - bounds.x1,
                bounds.y1 - bounds.y0,
                bounds.x1 - bounds.x0,
            ),
            _ => {
                return Err(ConvertError::Ocr(
                    "unsupported whole-page OCR rotation".into(),
                ));
            }
        };
        let convert = |value: f32, positive: bool| -> Result<u32, ConvertError> {
            let twips = (f64::from(value) * 20.0).round();
            if !twips.is_finite()
                || twips < 0.0
                || twips > f64::from(i32::MAX)
                || (positive && twips < 1.0)
            {
                return Err(ConvertError::Ocr(
                    "whole-page OCR frame is not representable".into(),
                ));
            }
            Ok(twips as u32)
        };
        Ok(Self {
            x: convert(x, false)? as i32,
            y: convert(y, false)? as i32,
            width: convert(width, true)?,
            height: convert(height, true)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 本文と位置がずれた内部状態を、部分的なframe出力にしない。
    #[test]
    fn mismatched_text_and_position_counts_are_rejected() {
        let page = ParsedPage {
            page_number: 0,
            width: 200.0,
            height: 300.0,
            rotation: 0,
        };
        assert!(Lines::positioned(vec!["body".into()], Vec::new(), &page).is_err());
    }

    /// 非有限値・負の幅・表現不能な座標は、整数への飽和変換で隠さない。
    #[test]
    fn unrepresentable_frames_are_rejected() {
        let page = ParsedPage {
            page_number: 0,
            width: 200.0,
            height: 300.0,
            rotation: 0,
        };
        for bounds in [
            ParsedBounds {
                x0: f32::NAN,
                y0: 20.0,
                x1: 50.0,
                y1: 40.0,
            },
            ParsedBounds {
                x0: 0.0,
                y0: 20.0,
                x1: f32::INFINITY,
                y1: 40.0,
            },
            ParsedBounds {
                x0: 50.0,
                y0: 20.0,
                x1: 10.0,
                y1: 40.0,
            },
            ParsedBounds {
                x0: 10.0,
                y0: 20.0,
                x1: 10.0,
                y1: 40.0,
            },
            ParsedBounds {
                x0: -10.0,
                y0: 20.0,
                x1: 20.0,
                y1: 40.0,
            },
            ParsedBounds {
                x0: 0.0,
                y0: 20.0,
                x1: f32::MAX,
                y1: 40.0,
            },
        ] {
            assert!(Frame::from_pdf(bounds, &page).is_err());
        }
        let page = ParsedPage {
            rotation: 45,
            ..page
        };
        assert!(
            Frame::from_pdf(
                ParsedBounds {
                    x0: 10.0,
                    y0: 20.0,
                    x1: 40.0,
                    y1: 50.0
                },
                &page
            )
            .is_err()
        );
    }

    proptest! {
        /// 任意DPIと四方向の回転でも、PDFを経由した行位置がtwips丸め1以内で戻る。
        #[test]
        fn raster_positions_roundtrip_to_page_frames(
            rotation in 0i32..4, dpi in 1u32..601,
            x in 0usize..100, y in 0usize..100,
            width in 1usize..100, height in 1usize..100,
        ) {
            let page = ParsedPage { page_number: 0, width: 400.0*72.0/dpi as f32,
                height: 600.0*72.0/dpi as f32, rotation: rotation*90 };
            let raster = PageOcrRaster { page: 0, width_px: 600, height_px: 600,
                region: Some([0,0,600,600]), text: Default::default() };
            let bounds = positioned_ocr::pdf_bounds(&page, raster, dpi, [x,y,x+width,y+height]).unwrap();
            let frame = Frame::from_pdf(bounds, &page).unwrap();
            for (actual, original) in [frame.x as i64,frame.y as i64,frame.width as i64,frame.height as i64]
                .into_iter().zip([x,y,width,height]) {
                let expected = (original as f64*1440.0/f64::from(dpi)).round() as i64;
                prop_assert!((actual-expected).abs()<=1);
            }
        }
    }
}
