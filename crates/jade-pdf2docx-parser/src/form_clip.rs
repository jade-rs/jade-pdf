//! Form の変換済み BBox を凸多角形として交差する。
use crate::{ParsedBounds, decode::DecodeBudget};
use std::borrow::Cow;

/// 外積計算で f32 座標の積をオーバーフローさせない点表現。
pub(crate) type Point = [f64; 2];

/// 呼び出し時に固定する可視領域。None は互換入力の未指定領域。
pub(crate) struct Clip {
    /// 反時計回りの頂点。空配列は不可視領域を表す。
    polygon: Option<Vec<Point>>,
}

impl Clip {
    /// 変換後の矩形の全頂点がclip内にあり、glyph分割を省略できるかを返す。
    pub(crate) fn contains_rect(&self, rect: [f32; 4], matrix: [f32; 6]) -> bool {
        let Some(polygon) = &self.polygon else {
            return true;
        };
        if polygon.is_empty() {
            return false;
        }
        [
            [rect[0], rect[1]],
            [rect[2], rect[1]],
            [rect[2], rect[3]],
            [rect[0], rect[3]],
        ]
        .iter()
        .all(|point| {
            let point = crate::content_clip::transform([point[0] as f64, point[1] as f64], matrix);
            polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .all(|(a, b)| {
                    (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]) >= 0.
                })
        })
    }

    /// 検証済みページ矩形から初期領域を作り、頂点の保存容量を確保前に予約する。
    pub(crate) fn page(
        budget: &DecodeBudget,
        media: Option<[f32; 4]>,
        crop: Option<[f32; 4]>,
    ) -> Option<Self> {
        if budget.is_stopped() {
            return None;
        }
        let clip = Self {
            polygon: match media {
                Some(r) => Some(rectangle(budget, r, [1., 0., 0., 1., 0., 0.])?),
                None => None,
            },
        };
        match crop {
            Some(crop) => clip.child(budget, crop, [1., 0., 0., 1., 0., 0.]),
            None => Some(clip),
        }
    }

    /// Form BBox を借用した親領域と交差し、内部 cm の影響を受けない領域を作る。
    /// 頂点・交差用配列の保存失敗では部分Clipを返さない。
    pub(crate) fn child(
        &self,
        budget: &DecodeBudget,
        bbox: [f32; 4],
        matrix: [f32; 6],
    ) -> Option<Self> {
        let polygon = rectangle(budget, bbox, matrix)?;
        self.intersect_polygon(budget, polygon)
    }

    /// ページ空間の凸多角形を現在領域と交差する。
    pub(crate) fn intersect_polygon(
        &self,
        budget: &DecodeBudget,
        polygon: Vec<Point>,
    ) -> Option<Self> {
        Some(Self {
            polygon: Some(match &self.polygon {
                // ページ内の親を切ることで巨大な BBox 辺の補間による桁落ちを避ける。
                Some(parent) => intersect(budget, Cow::Borrowed(parent), &polygon)?,
                None => polygon,
            }),
        })
    }

    /// 既存領域同士を交差し、Formへ通常path clipを継承する。
    pub(crate) fn intersection(&self, budget: &DecodeBudget, other: &Self) -> Option<Self> {
        let polygon = match (&self.polygon, &other.polygon) {
            (Some(left), Some(right)) => Some(intersect(budget, Cow::Borrowed(left), right)?),
            (left, right) => match left.as_ref().or(right.as_ref()) {
                Some(points) => {
                    let mut copy = budget.metadata_vector(points.len())?;
                    copy.extend_from_slice(points);
                    Some(copy)
                }
                None => None,
            },
        };
        Some(Self { polygon })
    }

    /// 正規化済み凸多角形を新しい可視領域にする。
    pub(crate) fn polygon(polygon: Vec<Point>) -> Self {
        Self {
            polygon: Some(polygon),
        }
    }

    /// テスト用のページ空間矩形を単位行列で切り抜く。
    #[cfg(test)]
    fn bounds(&self, bounds: ParsedBounds) -> Option<ParsedBounds> {
        self.transformed_bounds(
            &DecodeBudget::new(&crate::ParseLimits::default()),
            [bounds.x0, bounds.y0, bounds.x1, bounds.y1],
            [1., 0., 0., 1., 0., 0.],
        )
    }

    /// 局所文字矩形を四隅のまま切り抜き、面積のある交差の外接矩形を返す。
    pub(crate) fn transformed_bounds(
        &self,
        budget: &DecodeBudget,
        rect: [f32; 4],
        matrix: [f32; 6],
    ) -> Option<ParsedBounds> {
        self.transformed_bounds_with(budget, rect, matrix, None)
    }

    /// 文字矩形を二つの凸領域で順に切り、途中の外接矩形による過大評価を避ける。
    pub(crate) fn transformed_bounds_with(
        &self,
        budget: &DecodeBudget,
        rect: [f32; 4],
        matrix: [f32; 6],
        other: Option<&Self>,
    ) -> Option<ParsedBounds> {
        let polygon = rectangle(budget, rect, matrix)?;
        let polygon = match &self.polygon {
            Some(clip) => intersect(budget, Cow::Owned(polygon), clip)?,
            None => polygon,
        };
        let polygon = match other.and_then(|other| other.polygon.as_ref()) {
            Some(clip) => intersect(budget, Cow::Owned(polygon), clip)?,
            None => polygon,
        };
        if !has_area(&polygon) {
            return None;
        }
        let mut result = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for [x, y] in polygon {
            result[0] = result[0].min(x);
            result[1] = result[1].min(y);
            result[2] = result[2].max(x);
            result[3] = result[3].max(y);
        }
        Some(ParsedBounds {
            x0: result[0] as f32,
            y0: result[1] as f32,
            x1: result[2] as f32,
            y1: result[3] as f32,
        })
    }
}

/// 正規化した矩形を変換し、反転行列でも反時計回りに揃える。
fn rectangle(budget: &DecodeBudget, rect: [f32; 4], matrix: [f32; 6]) -> Option<Vec<Point>> {
    if budget.is_stopped() {
        return None;
    }
    if !rect.iter().chain(matrix.iter()).all(|v| v.is_finite()) {
        return Some(Vec::new());
    }
    let [a, b, c, d, e, f] = matrix.map(f64::from);
    let determinant = a * d - b * c;
    let [x0, y0, x1, y1] = rect.map(f64::from);
    let (x0, x1, y0, y1) = (x0.min(x1), x0.max(x1), y0.min(y1), y0.max(y1));
    if x0 == x1 || y0 == y1 || determinant == 0. {
        return Some(Vec::new());
    }
    let mut polygon = budget.metadata_vector(4)?;
    polygon.extend(
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
            .into_iter()
            .map(|[x, y]| [a * x + c * y + e, b * x + d * y + f]),
    );
    if determinant < 0. {
        polygon.reverse();
    }
    Some(polygon)
}

/// 有向辺の左側を正とする外積を返す。
fn side(start: Point, end: Point, point: Point) -> f64 {
    (end[0] - start[0]) * (point[1] - start[1]) - (end[1] - start[1]) * (point[0] - start[0])
}

/// 原点からの距離に依存せず、平行移動後も正の面積を判定する。
fn has_area(polygon: &[Point]) -> bool {
    let Some(&origin) = polygon.first() else {
        return false;
    };
    polygon
        .windows(2)
        .map(|edge| side(origin, edge[0], edge[1]))
        .sum::<f64>()
        > 0.
}

/// 凸領域の各半平面で切り、頂点数を Form 深度に比例する範囲に保つ。
/// 各辺の交差用配列と予想外の頂点増加も保存前に累積予約する。
fn intersect(
    budget: &DecodeBudget,
    mut polygon: Cow<'_, [Point]>,
    clip: &[Point],
) -> Option<Vec<Point>> {
    if budget.is_stopped() {
        return None;
    }
    if !has_area(clip) {
        return Some(Vec::new());
    }
    for index in 0..clip.len() {
        let (start, end) = (clip[index], clip[(index + 1) % clip.len()]);
        let Some(&last) = polygon.last() else {
            break;
        };
        let mut previous = last;
        let mut previous_side = side(start, end, previous);
        let mut output = budget.metadata_vector(polygon.len().saturating_add(1))?;
        for point in polygon.iter().copied() {
            let distance = side(start, end, point);
            if (distance >= 0.) != (previous_side >= 0.) {
                let ratio = previous_side / (previous_side - distance);
                let mut crossing = [
                    previous[0] + ratio * (point[0] - previous[0]),
                    previous[1] + ratio * (point[1] - previous[1]),
                ];
                // 軸平行辺の交点は既知の座標に揃え、境界を越える丸め誤差を防ぐ。
                if start[0] == end[0] {
                    crossing[0] = start[0];
                }
                if start[1] == end[1] {
                    crossing[1] = start[1];
                }
                crate::metadata_workspace::try_push(budget, &mut output, crossing)?;
            }
            if distance >= 0. {
                crate::metadata_workspace::try_push(budget, &mut output, point)?;
            }
            previous = point;
            previous_side = distance;
        }
        polygon = Cow::Owned(output);
    }
    Some(match polygon {
        Cow::Owned(polygon) => polygon,
        // 借用のまま終了するのは空の親だけで、複製用heapを確保しない。
        Cow::Borrowed(_) => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 幾何回帰では既定の容量予算を使う。
    fn budget() -> DecodeBudget {
        DecodeBudget::new(&crate::ParseLimits::default())
    }

    /// ページ矩形を既定容量で作る。
    fn page(media: Option<[f32; 4]>, crop: Option<[f32; 4]>) -> Clip {
        Clip::page(&budget(), media, crop).unwrap()
    }

    /// 座標配列を文字矩形にする。
    fn bounds([x0, y0, x1, y1]: [f32; 4]) -> ParsedBounds {
        ParsedBounds { x0, y0, x1, y1 }
    }

    /// 反転行列の winding と部分交差を確認する。
    #[test]
    fn reflected_rectangle_is_visible() {
        let clip = page(None, None)
            .child(&budget(), [0., 0., 10., 10.], [-1., 0., 0., 1., 100., 0.])
            .unwrap();
        assert_eq!(
            clip.bounds(bounds([85., 5., 95., 15.])),
            Some(bounds([90., 5., 95., 10.]))
        );
    }

    /// 有限 f32 の積が f32 範囲を超えてもページ内の交差を保持する。
    #[test]
    fn large_finite_coordinates_do_not_overflow_cross_products() {
        let clip = page(Some([0., 0., 100., 100.]), None)
            .child(
                &budget(),
                [-1e30, -1e30, 1e30, 1e30],
                [1e30, 0., 0., 1e30, 0., 0.],
            )
            .unwrap();
        assert_eq!(
            clip.bounds(bounds([20., 20., 30., 30.])),
            Some(bounds([20., 20., 30., 30.]))
        );
    }

    /// PBT の縮小入力で軸平行辺の交点が境界から漏れる誤差を固定する。
    #[test]
    fn axis_intersection_does_not_leak_beyond_zero_boundary() {
        let clip = page(Some([0., 0., 47., 69.]), None);
        assert_eq!(
            clip.bounds(bounds([-20., 0., 27., 69.])),
            Some(bounds([0., 0., 27., 69.]))
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]
        /// 矩形の解析解・包含性・同じ clip の再適用不変性を生成検証する。
        #[test]
        fn axis_aligned_intersection_matches_interval_arithmetic(
            x in -100i16..100, y in -100i16..100,
            width in 1u16..100, height in 1u16..100,
            offset_x in -100i16..100, offset_y in -100i16..100,
        ) {
            let (x, y, width, height, ox, oy) = (f32::from(x), f32::from(y), f32::from(width), f32::from(height), f32::from(offset_x), f32::from(offset_y));
            let clip = page(Some([x, y, x + width, y + height]), None);
            let text = bounds([x + ox, y + oy, x + ox + width, y + oy + height]);
            let expected = bounds([x.max(text.x0), y.max(text.y0), (x + width).min(text.x1), (y + height).min(text.y1)]);
            let actual = clip.bounds(text);
            if expected.x0 < expected.x1 && expected.y0 < expected.y1 {
                prop_assert_eq!(actual, Some(expected));
                prop_assert_eq!(clip.bounds(expected), actual);
            } else {
                prop_assert_eq!(actual, None);
            }
        }
    }
}

#[cfg(test)]
#[path = "form_clip_budget_tests.rs"]
mod budget_tests;
