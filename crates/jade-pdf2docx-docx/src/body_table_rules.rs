//! 本文の下線候補から、実在する閉じた原本表の外側水平辺だけを除く。
use super::*;

/// 原本文字を照合した表と四辺を確認し、本文の書式候補だけを絞る。
pub(super) fn remove_outer_frames(
    graphics: &mut Vec<ParsedGraphicElement>,
    tables: &[PositionedTable],
) {
    let frames = tables
        .iter()
        .filter(|table| {
            table.stroke_width.is_finite()
                && table.stroke_width > 0.
                && table.native_cells.iter().flatten().any(Option::is_some)
        })
        .filter_map(|table| {
            let bounds = normalized_bounds(table.bounds);
            closed(table.page, bounds, graphics).then_some((table.page, bounds))
        })
        .collect::<Vec<_>>();
    graphics.retain(|graphic| {
        !frames.iter().any(|(page, bounds)| {
            horizontal_edge(graphic, *page, bounds, bounds.y0)
                || horizontal_edge(graphic, *page, bounds, bounds.y1)
        })
    });
}

/// 整数gridとの端点差を確認し、対向辺の許容範囲が重なる小さい枠は推測しない。
fn closed(page: usize, bounds: ParsedBounds, graphics: &[ParsedGraphicElement]) -> bool {
    if ![bounds.x0, bounds.x1, bounds.y0, bounds.y1]
        .iter()
        .all(|v| v.is_finite())
        || bounds.x1 - bounds.x0 <= 2.
        || bounds.y1 - bounds.y0 <= 2.
    {
        return false;
    }
    [bounds.y0, bounds.y1].into_iter().all(|y| {
        graphics
            .iter()
            .any(|g| horizontal_edge(g, page, &bounds, y))
    }) && [bounds.x0, bounds.x1].into_iter().all(|x| {
        graphics.iter().any(|graphic| {
            line(graphic, page).is_some_and(|b| {
                is_vertical_line(&b)
                    && close(b.x0, x)
                    && close(b.y0, bounds.y0)
                    && close(b.y1, bounds.y1)
            })
        })
    })
}

/// 表の左右端と同じ範囲を持つ外側の水平線だけを識別する。
fn horizontal_edge(
    graphic: &ParsedGraphicElement,
    page: usize,
    frame: &ParsedBounds,
    y: f32,
) -> bool {
    line(graphic, page).is_some_and(|b| {
        is_horizontal_line(&b) && close(b.y0, y) && close(b.x0, frame.x0) && close(b.x1, frame.x1)
    })
}

/// 同じページの有限な実線だけを根拠にし、未知の線幅や座標は除外する。
fn line(graphic: &ParsedGraphicElement, page: usize) -> Option<ParsedBounds> {
    let b = normalized_bounds(graphic.bounds);
    (graphic.page == page
        && graphic.stroke_width.is_finite()
        && graphic.stroke_width > 0.
        && [b.x0, b.x1, b.y0, b.y1].iter().all(|v| v.is_finite()))
    .then_some(b)
}

/// 独立して量子化された原本の辺とgrid端点を、既存の1pt範囲で照合する。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 所有する原本の四辺を作り、後続のテストで一辺ずつ変える。
    fn frame(page: usize, b: ParsedBounds) -> Vec<ParsedGraphicElement> {
        [
            (b.x0, b.y0, b.x1, b.y0),
            (b.x0, b.y1, b.x1, b.y1),
            (b.x0, b.y0, b.x0, b.y1),
            (b.x1, b.y0, b.x1, b.y1),
        ]
        .into_iter()
        .map(|(x0, y0, x1, y1)| ParsedGraphicElement {
            page,
            bounds: ParsedBounds { x0, y0, x1, y1 },
            stroke_width: 1.,
        })
        .collect()
    }

    /// 元のframeは有限で、許容範囲が重ならない距離を持つ。
    fn bounds() -> ParsedBounds {
        ParsedBounds {
            x0: 10.,
            y0: 20.,
            x1: 210.,
            y1: 70.,
        }
    }

    /// 一辺でも欠ける場合や別ページの辺を、閉じたframeの根拠へ足さない。
    #[test]
    fn every_actual_edge_on_the_same_page_is_required() {
        let b = bounds();
        let source = frame(2, b);
        assert!(closed(2, b, &source));
        assert!(!closed(1, b, &source));
        for i in 0..4 {
            let mut missing = source.clone();
            missing.remove(i);
            assert!(!closed(2, b, &missing));
            let mut wrong = source.clone();
            wrong[i].page = 3;
            assert!(!closed(2, b, &wrong));
        }
    }

    /// 短い独立線や許容端点を越える線を、全幅の外辺と見なさない。
    #[test]
    fn partial_or_displaced_lines_are_not_outer_frame_edges() {
        let b = bounds();
        let mut source = frame(0, b);
        source[1].bounds.x1 -= 10.;
        assert!(!closed(0, b, &source));
        let mut source = frame(0, b);
        source[3].bounds.x0 += 2.;
        source[3].bounds.x1 += 2.;
        assert!(!closed(0, b, &source));
        let line = ParsedGraphicElement {
            page: 0,
            bounds: ParsedBounds {
                x0: 20.,
                y0: 70.,
                x1: 100.,
                y1: 70.,
            },
            stroke_width: 1.,
        };
        assert!(!horizontal_edge(&line, 0, &b, b.y1));
    }

    /// 非有限の座標・線幅と対向辺の区別できない小さいframeは未確認のままにする。
    #[test]
    fn invalid_or_ambiguous_frames_do_not_gain_confidence() {
        let b = bounds();
        for width in [0., -1., f32::NAN, f32::INFINITY] {
            let mut source = frame(0, b);
            source[0].stroke_width = width;
            assert!(!closed(0, b, &source));
        }
        let mut source = frame(0, b);
        source[2].bounds.y0 = f32::NAN;
        assert!(!closed(0, b, &source));
        let tiny = ParsedBounds { x1: 11., ..b };
        assert!(!closed(0, tiny, &frame(0, tiny)));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 表の移動・size・ページ番号を変えても、四辺と独立線の役割は変わらない。
        #[test]
        fn closed_frame_proof_is_covariant(x in -100_f32..100.,y in -100_f32..100.,width in 10_f32..300.,height in 10_f32..200.,page in 0_usize..10) {
            let b=ParsedBounds{x0:x,y0:y,x1:x+width,y1:y+height};let mut source=frame(page,b);
            prop_assert!(closed(page,b,&source));source.push(ParsedGraphicElement{page,bounds:ParsedBounds{x0:x+2.,y0:y+height+3.,x1:x+width-2.,y1:y+height+3.},stroke_width:1.});
            prop_assert!(closed(page,b,&source));prop_assert!(!horizontal_edge(source.last().unwrap(),page,&b,b.y1));
        }
    }
}
