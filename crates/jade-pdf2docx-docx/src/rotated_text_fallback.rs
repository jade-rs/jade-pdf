//! 直角回転した非正立runを横書きへ戻す既存fallbackで、右寄せの単語幅を潰さない。
use super::{ParagraphAlignment, ParsedDocument, StructuredTextPage};

/// 縦長のsource advanceが確認できる右寄せ単一runに、横書きfallback用の左端を確保する。
pub(super) fn left_x(parsed: &ParsedDocument, page: &StructuredTextPage) -> Option<f32> {
    let frame = parsed.pages.iter().find(|p| p.page_number == page.page)?;
    if frame.rotation.rem_euclid(180) != 90 {
        return None;
    }
    let (width, height) = super::parsed_page_display_size_points(frame);
    if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
        return None;
    }
    parsed
        .text_elements
        .iter()
        .filter(|t| t.page == page.page)
        .filter_map(|t| {
            let b = t.bounds;
            if ![b.x0, b.x1, b.y0, b.y1, t.font_size]
                .into_iter()
                .all(f32::is_finite)
                || t.font_size <= 0.
                || b.x0 < 0.
                || b.y0 < 0.
                || b.x1 > width
                || b.y1 > height
            {
                return None;
            }
            let width = b.x1 - b.x0;
            let advance = b.y1 - b.y0;
            if width <= 0.
                || advance <= width
                || advance <= t.font_size
                || (t.baseline_y == Some(b.y0) && (advance - t.font_size).abs() < 0.001)
            {
                return None;
            }
            // 同じ文字列だけで別行へ結び付けず、実際のsource行の左右端も照合する。
            page.lines
                .iter()
                .any(|line| {
                    line.alignment == ParagraphAlignment::Right
                        && line.runs.len() == 1
                        && line.text == t.text
                        && line.left_x == b.x0
                        && line.right_x == b.x1
                })
                .then_some((b.x1 - advance).max(0.))
        })
        .reduce(f32::min)
}
