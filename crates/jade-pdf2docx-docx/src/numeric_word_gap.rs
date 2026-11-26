//! 横幅を縮めた別々の数値欄を、本文runの結合で一つの値へ変えない。
use jade_pdf2docx_parser::ParsedTextElement;

/// 同じsizeと正立baselineを持つ数値欄に、解析側と同じ0.2em以上の語間を保持する。
pub(super) fn needs_separator(
    left: &ParsedTextElement,
    right: &ParsedTextElement,
    gap: f32,
) -> bool {
    let size = left.font_size.min(right.font_size);
    let (Some(left_y), Some(right_y)) = (left.baseline_y, right.baseline_y) else {
        return false;
    };
    gap.is_finite()
        && left.font_size.is_finite()
        && right.font_size.is_finite()
        && size > 0.
        && (left.font_size - right.font_size).abs() < 0.001
        && left_y.is_finite()
        && right_y.is_finite()
        && (left_y - right_y).abs() <= size * 0.05
        && gap >= size * 0.2
        && !left.text.ends_with(char::is_whitespace)
        && !right.text.starts_with(char::is_whitespace)
        && super::text_row_segments::numeric_pair(&left.text, &right.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jade_pdf2docx_parser::ParsedBounds;

    /// 所有する同じsize・baselineの数値二欄を作る。
    fn field(text: &str) -> ParsedTextElement {
        ParsedTextElement {
            fill_stroke_width: None,
            page: 0,
            text: text.into(),
            font_name: None,
            font_size: 10.,
            font_weight: None,
            fill_color: None,
            baseline_y: Some(100.),
            bounds: ParsedBounds {
                x0: 20.,
                x1: 38.,
                y0: 100.,
                y1: 110.,
            },
        }
    }

    /// 非有限・小さいtracking・不明baseline・異なるsize・散文には新しい空白を補わない。
    #[test]
    fn uncertain_numeric_geometry_or_existing_separator_is_not_reinterpreted() {
        let left = field("3");
        let right = field("768");
        assert!(needs_separator(&left, &right, 2.));
        for gap in [1.99, 0., -10., f32::NAN, f32::INFINITY] {
            assert!(!needs_separator(&left, &right, gap));
        }
        for baseline in [None, Some(f32::NAN), Some(101.)] {
            let mut invalid = right.clone();
            invalid.baseline_y = baseline;
            assert!(!needs_separator(&left, &invalid, 5.));
        }
        for size in [0., -10., 6., f32::NAN, f32::INFINITY] {
            let mut invalid = right.clone();
            invalid.font_size = size;
            assert!(!needs_separator(&left, &invalid, 5.));
        }
        for (a, b) in [("3 ", "768"), ("3", " 768"), ("Code", "768"), ("3", "本文")] {
            assert!(!needs_separator(&field(a), &field(b), 5.));
        }
    }
}
