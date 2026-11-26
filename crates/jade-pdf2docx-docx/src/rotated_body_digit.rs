//! 直角回転した本文字形の間の数字を、下端だけを根拠にページ番号として消さない。
use super::{ParsedBounds, ParsedDocument, StructuredTextLine};

/// 同じ回転列の上下に近接した本文字形があるときだけ、数字を本文へ保持する。
pub(super) fn is_between_body_glyphs(
    parsed: &ParsedDocument,
    page: usize,
    line: &StructuredTextLine,
) -> bool {
    let mut pages = parsed
        .pages
        .iter()
        .filter(|frame| frame.page_number == page);
    let Some(frame) = pages.next() else {
        return false;
    };
    if pages.next().is_some() || frame.rotation.rem_euclid(180) != 90 {
        return false;
    }
    parsed.text_elements.iter().any(|digit| {
        if digit.page != page
            || digit.text.trim() != line.text.trim()
            || digit.baseline_y.is_some()
            || digit.bounds.x0 != line.left_x
            || digit.bounds.x1 != line.right_x
            || digit.bounds.y1 != line.top_y
            || !usable(digit.bounds, digit.font_size)
            || digit.font_name.is_none()
        {
            return false;
        }
        let x = (digit.bounds.x0 + digit.bounds.x1) / 2.;
        let mut before = false;
        let mut after = false;
        for other in &parsed.text_elements {
            if other.page != page
                || other.baseline_y.is_some()
                || other.font_name != digit.font_name
                || other.font_size < digit.font_size
                || !usable(other.bounds, other.font_size)
                || !other.text.chars().any(char::is_alphabetic)
                || x < other.bounds.x0
                || x > other.bounds.x1
            {
                continue;
            }
            let tolerance = other.font_size * 0.5;
            let lower_gap = digit.bounds.y0 - other.bounds.y1;
            let upper_gap = other.bounds.y0 - digit.bounds.y1;
            before |= lower_gap >= 0. && lower_gap <= tolerance;
            after |= upper_gap >= 0. && upper_gap <= tolerance;
            if before && after {
                return true;
            }
        }
        false
    })
}

/// 有限で正の字面と文字サイズだけを、近接した本文字形の証明へ使う。
fn usable(bounds: ParsedBounds, size: f32) -> bool {
    [bounds.x0, bounds.x1, bounds.y0, bounds.y1, size]
        .into_iter()
        .all(f32::is_finite)
        && bounds.x1 > bounds.x0
        && bounds.y1 > bounds.y0
        && size > 0.
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParagraphAlignment;

    /// 正式PDFから、回転後の数字とその前後の本文字形を得る。
    fn sample() -> (ParsedDocument, StructuredTextLine) {
        let source = fixture::pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 400 600] /Rotate 270 >>".into(),
            "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>"
                .into(),
            fixture::stream(
                "BT /F 18 Tf 1 0 0 1 30 80 Tm (A) Tj /F 10 Tf 1 0 0 1 41 86 Tm (2) Tj /F 18 Tf 1 0 0 1 48 80 Tm (B) Tj ET",
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
        ]);
        let parsed =
            jade_pdf2docx_parser::parse_pdf_bytes("owned-bracketed-digit.pdf", &source).unwrap();
        let digit = parsed.text_elements.iter().find(|t| t.text == "2").unwrap();
        let line = StructuredTextLine {
            top_y: digit.bounds.y1,
            left_x: digit.bounds.x0,
            right_x: digit.bounds.x1,
            alignment: ParagraphAlignment::Left,
            runs: Vec::new(),
            split_from_row: false,
            text: "2".into(),
        };
        (parsed, line)
    }

    /// 片側だけの文字・別のfont・離れた列・既知の正立文字を根拠に数字を本文と断定しない。
    #[test]
    fn both_nearby_matching_body_glyphs_are_required() {
        let (parsed, line) = sample();
        assert!(is_between_body_glyphs(&parsed, 0, &line));
        for missing in ["A", "B"] {
            let mut changed = parsed.clone();
            changed.text_elements.retain(|t| t.text != missing);
            assert!(!is_between_body_glyphs(&changed, 0, &line));
        }
        for variant in 0..3 {
            let mut changed = parsed.clone();
            let a = changed
                .text_elements
                .iter_mut()
                .find(|t| t.text == "A")
                .unwrap();
            match variant {
                0 => a.font_name = Some("Different".into()),
                1 => a.bounds.x1 = line.left_x - 1.,
                _ => a.baseline_y = Some(80.),
            }
            assert!(!is_between_body_glyphs(&changed, 0, &line));
        }
    }

    /// 未確認の枠・非有限の字面・不正文字サイズを、本文の近接証明に使わない。
    #[test]
    fn ambiguous_frames_and_invalid_geometry_do_not_gain_a_body_proof() {
        let (parsed, line) = sample();
        let mut changed = parsed.clone();
        changed.pages.push(changed.pages[0].clone());
        assert!(!is_between_body_glyphs(&changed, 0, &line));
        for rotation in [0, 45, 180] {
            let mut changed = parsed.clone();
            changed.pages[0].rotation = rotation;
            assert!(!is_between_body_glyphs(&changed, 0, &line));
        }
        for size in [0., f32::NAN, f32::INFINITY] {
            let mut changed = parsed.clone();
            changed
                .text_elements
                .iter_mut()
                .find(|t| t.text == "2")
                .unwrap()
                .font_size = size;
            assert!(!is_between_body_glyphs(&changed, 0, &line));
        }
    }
}
