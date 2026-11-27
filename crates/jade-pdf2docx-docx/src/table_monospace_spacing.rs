//! 原本の等幅文字の前進幅から、連続する文字と一文字の語間空白を区別する。
use jade_pdf2docx_parser::ParsedTextElement;

/// 同じ基線にあるCourierの隣り合う二文字から確認できた空白。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Spacing {
    /// 先行文字の前進幅の直後に次の文字がある。
    Adjacent,
    /// 一文字の前進幅がそのまま空いている。
    OneSpace,
}

/// Courierの標準前進幅を確認した二文字の、隣接または一文字分の語間を返す。
pub(super) fn between(left: &ParsedTextElement, right: &ParsedTextElement) -> Option<Spacing> {
    if left.page != right.page
        || !ascii_letter_or_digit(left)
        || !ascii_letter_or_digit(right)
        || !matches!(
            left.font_name.as_deref(),
            Some("Courier" | "Courier-Bold" | "Courier-Oblique" | "Courier-BoldOblique")
        )
        || left.font_name != right.font_name
        || left.font_size != right.font_size
        || !left.font_size.is_finite()
        || left.font_size <= 0.0
    {
        return None;
    }
    let baseline = left.baseline_y.filter(|value| value.is_finite())?;
    // Wordの1 twipの半分を上限とし、小さい文字では前進幅に対する許容幅も絞る。
    let tolerance = (left.font_size * 0.0025).min(0.025);
    let advance = left.font_size * 0.6;
    let verified = [left, right].into_iter().all(|element| {
        let b = element.bounds;
        element.baseline_y == Some(baseline)
            && b.y0 == baseline
            && (b.y1 - b.y0 - element.font_size).abs() <= tolerance
            && b.x1 > b.x0
            && (b.x1 - b.x0 - advance).abs() <= tolerance
    });
    if !verified {
        return None;
    }

    let gap = right.bounds.x0 - left.bounds.x1;
    if gap.abs() <= tolerance {
        Some(Spacing::Adjacent)
    } else if (gap - advance).abs() <= tolerance {
        Some(Spacing::OneSpace)
    } else {
        None
    }
}

/// 従来の自動空白挿入の対象となる、ASCIIの一文字断片を確認する。
fn ascii_letter_or_digit(element: &ParsedTextElement) -> bool {
    matches!(element.text.as_bytes(), [byte] if byte.is_ascii_alphanumeric())
}
