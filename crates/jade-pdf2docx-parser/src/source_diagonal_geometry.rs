//! 対角strokeの有限幾何・clip・後続描画・表示原点を、走査から分けて検証する。
use super::{ParsedPageViewport, State};
use crate::{
    ParsedBounds, ParsedSourceDiagonal, decode::DecodeBudget, metadata_workspace::try_push,
};

/// 一様な変換だけで、方向に依存しない実効線幅を求める。
pub(super) fn width(state: State) -> Option<f32> {
    let [a, b, c, d, _, _] = state.matrix.map(f64::from);
    let x = a * a + b * b;
    let y = c * c + d * d;
    let w = f64::from(state.width) * x.sqrt();
    (state.matrix.iter().all(|v| v.is_finite())
        && state.cap.is_some()
        && x > 0.
        && x == y
        && a * c + b * d == 0.
        && w.is_finite()
        && w > 0.
        && w <= f64::from(f32::MAX))
    .then_some(w as f32)
    .filter(|w| w.is_finite() && *w > 0.)
}

/// 点の方向を保ったままf64で変換し、非有限・表現不能の結果を使わない。
pub(super) fn point(matrix: [f32; 6], x: f32, y: f32) -> Option<[f32; 2]> {
    let p = crate::content_clip::transform([f64::from(x), f64::from(y)], matrix);
    p.iter()
        .all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX))
        .then(|| p.map(|v| v as f32))
}

/// 複数subpathの後続fillでも、隠し得る全領域の外接矩形を追跡する。
pub(super) fn grow(bounds: &mut Option<ParsedBounds>, p: [f32; 2]) {
    if let Some(b) = bounds {
        b.x0 = b.x0.min(p[0]);
        b.x1 = b.x1.max(p[0]);
        b.y0 = b.y0.min(p[1]);
        b.y1 = b.y1.max(p[1]);
    } else {
        *bounds = Some(ParsedBounds {
            x0: p[0],
            y0: p[1],
            x1: p[0],
            y1: p[1],
        });
    }
}

/// 後続の塗りが重なる候補を除き、画像や未知の領域では既存候補を保守的に捨てる。
pub(super) fn occlude(
    output: &mut Vec<ParsedSourceDiagonal>,
    page: usize,
    bounds: Option<ParsedBounds>,
) {
    output.retain(|line| {
        if line.page != page {
            return true;
        }
        let Some(b) = bounds else {
            return false;
        };
        let pad = line.stroke_width;
        line.start[0].max(line.end[0]) + pad < b.x0
            || line.start[0].min(line.end[0]) - pad > b.x1
            || line.start[1].max(line.end[1]) + pad < b.y0
            || line.start[1].min(line.end[1]) - pad > b.y1
    });
}

/// 描画前のclipとページ内へstroke全体が収まる、非軸平行の線分だけを保存する。
#[allow(clippy::too_many_arguments)]
pub(super) fn emit(
    budget: &DecodeBudget,
    output: &mut Vec<ParsedSourceDiagonal>,
    page: usize,
    segment: ([f32; 2], [f32; 2]),
    stroke_width: f32,
    clip: Option<&crate::form_clip::Clip>,
    page_clip: &crate::form_clip::Clip,
) -> Option<()> {
    let (start, end) = segment;
    if start[0] == end[0] || start[1] == end[1] {
        return Some(());
    }
    // square capの斜方向への張り出しも包含する保守的な全線幅padding。
    let pad = stroke_width;
    let padded = [
        start[0].min(end[0]) - pad,
        start[1].min(end[1]) - pad,
        start[0].max(end[0]) + pad,
        start[1].max(end[1]) + pad,
    ];
    if padded.iter().all(|v| v.is_finite())
        && page_clip.contains_rect(padded, [1., 0., 0., 1., 0., 0.])
        && clip.is_none_or(|c| c.contains_rect(padded, [1., 0., 0., 1., 0., 0.]))
    {
        try_push(
            budget,
            output,
            ParsedSourceDiagonal {
                page,
                start,
                end,
                stroke_width,
            },
        )?;
    }
    Some(())
}

/// 原点移動とcardinal page rotationで、外接矩形へ向きを潰さず二端点を変換する。
pub(in crate::parser) fn in_viewports(
    lines: &mut Vec<ParsedSourceDiagonal>,
    viewports: &[ParsedPageViewport],
) {
    lines.retain_mut(|line| {
        let Some(v) = viewports.iter().find(|v| v.page.page_number == line.page) else {
            return false;
        };
        for p in [&mut line.start, &mut line.end] {
            let x = p[0] - v.origin_x;
            let y = p[1] - v.origin_y;
            *p = match v.page.rotation.rem_euclid(360) {
                90 => [y, v.page.width - x],
                180 => [v.page.width - x, v.page.height - y],
                270 => [v.page.height - y, x],
                _ => [x, y],
            };
        }
        line.start.iter().chain(&line.end).all(|v| v.is_finite())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// min/maxでNaNが有限な相方へ隠れても、未知の元端点を結果へ保存しない。
    #[test]
    fn raw_nonfinite_points_cannot_be_hidden_by_bounds_normalization() {
        let budget = DecodeBudget::default();
        let page = crate::form_clip::Clip::page(&budget, None, None).unwrap();
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for i in 0..4 {
                let mut points = [[100., 200.], [200., 100.]];
                points[i / 2][i % 2] = invalid;
                let mut output = Vec::new();
                assert!(
                    emit(
                        &budget,
                        &mut output,
                        0,
                        (points[0], points[1]),
                        0.5,
                        None,
                        &page
                    )
                    .is_some()
                );
                assert!(
                    output.is_empty(),
                    "raw nonfinite coordinate{i} must remain unknown"
                );
            }
        }
    }
}
