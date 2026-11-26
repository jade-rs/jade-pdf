//! セル所有を決めた整数gridと、寸法へ渡す一意な原本小数座標を区別する。
use super::*;

/// 既存gridの各境界へ一意に対応する、上書きしていない原本罫線座標。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Grid {
    /// 左から右への原本縦罫線座標。
    pub(super) xs: Vec<f32>,
    /// 下から上への原本横罫線座標。
    pub(super) ys: Vec<f32>,
}

/// 同じ整数キーを持つ異なる原本境界があれば、寸法全体を従来経路へ戻す。
pub(super) fn grid(graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> Option<Grid> {
    let x = edges(graphics, xs, ys, true)?;
    let y = edges(graphics, ys, xs, false)?;
    for axis in [&x, &y] {
        let start = f64::from(axis[0]);
        if (f64::from(*axis.last()?) - start) * 20. > f64::from(i32::MAX)
            || axis.windows(2).any(|p| {
                ((f64::from(p[1]) - start) * 20.).round()
                    <= ((f64::from(p[0]) - start) * 20.).round()
            })
        {
            return None;
        }
    }
    if x.iter().zip(xs).all(|(a, b)| *a == *b as f32)
        && y.iter().zip(ys).all(|(a, b)| *a == *b as f32)
    {
        return None;
    }
    Some(Grid { xs: x, ys: y })
}

/// 同じページの既存連結成分内で、各整数境界の原本軸座標をただ一つ照合する。
fn edges(
    graphics: &[ParsedGraphicElement],
    keys: &[i32],
    cross: &[i32],
    vertical: bool,
) -> Option<Vec<f32>> {
    let lo = *cross.first()? as f32;
    let hi = *cross.last()? as f32;
    if keys.len() < 2 || lo >= hi {
        return None;
    }
    let values = keys
        .iter()
        .map(|key| {
            let mut value = None;
            for g in graphics {
                let b = g.bounds;
                if ![b.x0, b.y0, b.x1, b.y1, g.stroke_width]
                    .iter()
                    .all(|v| v.is_finite())
                    || g.stroke_width <= 0.
                {
                    continue;
                }
                let (fixed, end, start, stop) = if vertical {
                    (b.x0, b.x1, b.y0, b.y1)
                } else {
                    (b.y0, b.y1, b.x0, b.x1)
                };
                if fixed != end
                    || fixed.round() as f64 != f64::from(*key)
                    || start.max(stop) <= lo
                    || start.min(stop) >= hi
                {
                    continue;
                }
                if value.is_some_and(|old| old != fixed) {
                    return None;
                }
                value = Some(fixed);
            }
            value
        })
        .collect::<Option<Vec<_>>>()?;
    values
        .windows(2)
        .all(|p| p[1] > p[0] && f64::from(p[1]) - f64::from(p[0]) <= f64::from(i32::MAX) / 20.)
        .then_some(values)
}

impl Grid {
    /// 原本の表矩形を返し、Word配置も整数キーから独立させる。
    pub(super) fn bounds(&self) -> ParsedBounds {
        ParsedBounds {
            x0: self.xs[0],
            x1: *self.xs.last().unwrap(),
            y0: self.ys[0],
            y1: *self.ys.last().unwrap(),
        }
    }

    /// 最初の境界からの累積twipを丸め、列の合計幅へ丸め誤差を蓄積しない。
    pub(super) fn widths(&self) -> Vec<usize> {
        let left = f64::from(self.xs[0]);
        self.xs
            .windows(2)
            .map(|p| {
                (((f64::from(p[1]) - left) * 20.).round()
                    - ((f64::from(p[0]) - left) * 20.).round()) as usize
            })
            .collect()
    }

    /// 原本上端からの累積高へ既存の薄罫線補正を配分し、一度だけtwipへ丸める。
    pub(super) fn heights(&self, extra: f32) -> Option<Vec<f32>> {
        if !extra.is_finite() || extra < 0. {
            return None;
        }
        let top = f64::from(*self.ys.last()?);
        let count = self.ys.len() - 1;
        self.ys
            .windows(2)
            .rev()
            .enumerate()
            .map(|(i, p)| {
                let start = ((top - f64::from(p[1]) + f64::from(extra) * i as f64 / count as f64)
                    * 20.)
                    .round();
                let end = ((top - f64::from(p[0])
                    + f64::from(extra) * (i + 1) as f64 / count as f64)
                    * 20.)
                    .round();
                let height = end - start;
                (height > 0. && height <= f64::from(i32::MAX)).then_some(height as f32)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 端点の丸めが同じtwipへ潰れるほど細い領域をWordセル幅へ渡さない。
    #[test]
    fn sub_twip_or_unresolved_grid_uses_fallback() {
        let line = |x| ParsedGraphicElement {
            page: 0,
            bounds: ParsedBounds {
                x0: x,
                x1: x,
                y0: 10.,
                y1: 20.,
            },
            stroke_width: 1.,
        };
        let mut graphics = vec![line(0.4999), line(0.5001)];
        for y in [10., 20.] {
            graphics.push(ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 0.,
                    x1: 1.,
                    y0: y,
                    y1: y,
                },
                stroke_width: 1.,
            });
        }
        assert!(grid(&graphics, &[0, 1], &[10, 20]).is_none());
        assert!(grid(&[], &[0, 20], &[10, 20]).is_none());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 累積境界の丸めで、各列端と全幅の誤差を半twip以内へ保つ。
        #[test]
        fn cumulative_widths_preserve_each_source_endpoint(gaps in proptest::collection::vec(105u16..999,1..20), offset in -100i16..100) {
            let mut xs=vec![f32::from(offset)/10.];
            for gap in gaps {xs.push(xs.last().unwrap()+f32::from(gap)/100.);}
            let grid=Grid {xs,ys:vec![0.,10.]};
            let mut actual=0usize;
            for (width,edge) in grid.widths().into_iter().zip(grid.xs.iter().skip(1)) {
                prop_assert!(width>0);actual+=width;
                let expected=(f64::from(*edge)-f64::from(grid.xs[0]))*20.;
                prop_assert!((actual as f64-expected).abs()<=0.50001);
            }
            let heights=grid.heights(0.).unwrap();prop_assert_eq!(heights,vec![200.]);
        }
    }
}
