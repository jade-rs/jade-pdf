//! 本文領域の原点に依存しないheader/footerの水平基準位置を作る。
use super::{ParagraphAlignment, StructuredTextLine};
use docx_rs::PageMargin;

/// ページ内の原本座標を、実section余白からの符号つきindentへ変換する。
/// 中央行の書き込み幅を保ち、左右の行は本文余白内へ押し込めない。
pub(super) fn indents(
    line: &StructuredTextLine,
    width_twips: u32,
    margin: &PageMargin,
) -> Option<(i32, i32)> {
    let left = f64::from(line.left_x) * 20.;
    let right = f64::from(line.right_x) * 20.;
    let width = f64::from(width_twips);
    let body_left = f64::from(margin.left);
    let body_right = width - f64::from(margin.right);
    if !left.is_finite()
        || !right.is_finite()
        || left < 0.
        || right <= left
        || right > width + 0.5
        || margin.left < 0
        || margin.right < 0
        || body_right <= body_left
        || margin.gutter != 0
    {
        return None;
    }
    let (indent_left, indent_right) = match line.alignment {
        ParagraphAlignment::Left => (
            left.round() - body_left,
            (body_right - right.round()).min(0.),
        ),
        ParagraphAlignment::Right => (
            (left.round() - body_left).min(0.),
            body_right - right.round(),
        ),
        ParagraphAlignment::Center => {
            let shift = ((left + right - body_left - body_right) / 2.).round();
            (shift, -shift)
        }
    };
    [indent_left, indent_right]
        .iter()
        .all(|value| *value >= f64::from(i32::MIN) && *value <= f64::from(i32::MAX))
        .then_some((indent_left as i32, indent_right as i32))
}
