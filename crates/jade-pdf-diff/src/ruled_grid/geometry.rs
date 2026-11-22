//! 軸に平行な罫線の材料の接触から、有限の閉領域だけを取り出す。
use crate::Bounds;
use std::collections::{BTreeMap, BTreeSet};

/// ページ座標に置かれた、太さを持つ一本の罫線。
#[derive(Clone, Copy, Debug)]
pub(super) struct Line {
    /// 水平なら真、垂直なら偽。
    pub horizontal: bool,
    /// 水平なら高さ、垂直なら横位置。
    pub coordinate: f64,
    /// 長軸の始まり。
    pub start: f64,
    /// 長軸の終わり。
    pub end: f64,
    /// 実際に塗られた材料の短軸の幅。
    pub thickness: f64,
}

/// 罫線グラフの有限な一面。結合セルの凹形状も保持する。
#[derive(Debug)]
pub(crate) struct Face {
    /// 面の周囲の頂点。最後の辺は先頭へ戻る。
    pub polygon: Vec<(f64, f64)>,
    /// 頂点すべてを覆う矩形。
    pub bounds: Bounds,
}

impl Face {
    /// 壁上の点を除いて面の内部を判定し、結合セルの凹みを矩形で埋めない。
    pub(super) fn contains(&self, x: f64, y: f64) -> bool {
        if !x.is_finite() || !y.is_finite() || x.abs() > 1e9 || y.abs() > 1e9 {
            return false;
        }
        if x <= self.bounds.x0 || x >= self.bounds.x1 || y <= self.bounds.y0 || y >= self.bounds.y1
        {
            return false;
        }
        let p = Point::new(x, y);
        let points: Vec<_> = self
            .polygon
            .iter()
            .map(|&(x, y)| Point::new(x, y))
            .collect();
        let mut inside = false;
        for (a, b) in points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
        {
            if a.x == b.x && p.x == a.x && a.y.min(b.y) <= p.y && p.y <= a.y.max(b.y)
                || a.y == b.y && p.y == a.y && a.x.min(b.x) <= p.x && p.x <= a.x.max(b.x)
            {
                return false;
            }
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x {
                inside = !inside;
            }
        }
        inside
    }
}

/// 元の線の微小な丸め差だけをまとめる座標の分解能。
const SCALE: f64 = 4096.0;
/// 任意入力で頂点の保存量が増えすぎない上限。
const MAX_POINTS: usize = 32_768;
/// 任意入力で半辺の保存量が増えすぎない上限。
const MAX_EDGES: usize = 65_536;

/// 丸め誤差で同じ頂点が別々にならない、有限範囲の座標。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Point {
    /// 固定分解能で表した横位置。
    x: i64,
    /// 固定分解能で表した縦位置。
    y: i64,
}
impl Point {
    /// 事前に有限範囲と確認した二つの値を頂点へ変換する。
    fn new(x: f64, y: f64) -> Self {
        Self {
            x: (x * SCALE).round() as i64,
            y: (y * SCALE).round() as i64,
        }
    }
    /// ページ座標へ戻す。
    fn values(self) -> (f64, f64) {
        (self.x as f64 / SCALE, self.y as f64 / SCALE)
    }
}

/// 処理を打ち切るときは、残る予算をゼロにして部分結果を使わせない。
fn charge(work: &mut usize, amount: usize) -> Option<()> {
    if amount > *work {
        *work = 0;
        None
    } else {
        *work -= amount;
        Some(())
    }
}

/// 近接した同一直線の座標だけをまとめ、連鎖で離れた罫線を潰さない。
fn snap(lines: &mut [Line], horizontal: bool) {
    let mut values: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.horizontal == horizontal)
        .map(|(i, l)| (i, l.coordinate))
        .collect();
    values.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut at = 0;
    while at < values.len() {
        let end = at + values[at..].partition_point(|(_, c)| *c - values[at].1 <= 0.002);
        let value = (values[(at + end) / 2].1 * SCALE).round() / SCALE;
        for &(i, _) in &values[at..end] {
            lines[i].coordinate = value;
        }
        at = end;
    }
}

/// 二つの軸平行な辺が接続する向きを、東・北・西・南の順に返す。
fn direction(a: Point, b: Point) -> Option<usize> {
    match (b.x - a.x, b.y - a.y) {
        (dx, 0) if dx > 0 => Some(0),
        (0, dy) if dy > 0 => Some(1),
        (dx, 0) if dx < 0 => Some(2),
        (0, dy) if dy < 0 => Some(3),
        _ => None,
    }
}

/// 右側の面をたどり、外側の無限領域や面積ゼロの往復を除く。
fn trace(graph: &BTreeMap<Point, BTreeSet<Point>>, work: &mut usize) -> Option<Vec<Face>> {
    let edges: usize = graph.values().map(BTreeSet::len).sum();
    if edges > MAX_EDGES {
        return None;
    }
    let mut next = BTreeMap::new();
    for (&p, neighbors) in graph {
        let mut directions = [None; 4];
        for &q in neighbors {
            let d = direction(p, q)?;
            if directions[d].replace(q).is_some() {
                return None;
            }
        }
        next.insert(p, directions);
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (&first, neighbors) in graph {
        for &second in neighbors {
            if seen.contains(&(first, second)) {
                continue;
            }
            let mut polygon = Vec::new();
            let (mut p, mut q) = (first, second);
            loop {
                charge(work, 1)?;
                if seen.contains(&(p, q)) {
                    if (p, q) != (first, second) {
                        return None;
                    }
                    break;
                }
                seen.insert((p, q));
                polygon.push(p);
                if polygon.len() > edges {
                    return None;
                }
                let d = direction(p, q)?;
                let outgoing = next.get(&q)?;
                let following = [(d + 3) % 4, d, (d + 1) % 4, (d + 2) % 4]
                    .into_iter()
                    .find_map(|i| outgoing[i])?;
                p = q;
                q = following;
            }
            let origin = polygon[0];
            let area: i128 = polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .take(polygon.len())
                .map(|(a, b)| {
                    i128::from(a.x - origin.x) * i128::from(b.y - origin.y)
                        - i128::from(b.x - origin.x) * i128::from(a.y - origin.y)
                })
                .sum();
            // 一平方ポイント未満の継ぎ目は、文字を所属させるセルにしない。
            if area >= -(2.0 * SCALE * SCALE) as i128 {
                continue;
            }
            let points: Vec<_> = polygon.into_iter().map(Point::values).collect();
            let bounds = points.iter().fold(
                Bounds {
                    x0: f64::INFINITY,
                    y0: f64::INFINITY,
                    x1: f64::NEG_INFINITY,
                    y1: f64::NEG_INFINITY,
                },
                |b, &(x, y)| Bounds {
                    x0: b.x0.min(x),
                    y0: b.y0.min(y),
                    x1: b.x1.max(x),
                    y1: b.y1.max(y),
                },
            );
            result.push(Face {
                polygon: points,
                bounds,
            });
        }
    }
    Some(result)
}

/// 予算が不足したときは部分的なセル一覧を返さず、推定を中止する。
pub(super) fn faces(input: &[Line], work: &mut usize) -> Option<Vec<Face>> {
    if input.len() > 2048 {
        return None;
    }
    charge(
        work,
        input
            .len()
            .saturating_mul(input.len())
            .saturating_add(input.len().saturating_mul(32)),
    )?;
    if input.iter().any(|l| {
        ![l.coordinate, l.start, l.end, l.thickness]
            .into_iter()
            .all(f64::is_finite)
            || l.start >= l.end
            || !(0.0..=8.0).contains(&l.thickness)
            || [l.coordinate, l.start, l.end]
                .into_iter()
                .any(|c| c.abs() > 1e9)
    }) {
        return None;
    }
    let mut lines = input.to_vec();
    snap(&mut lines, true);
    snap(&mut lines, false);
    let mut vertices: Vec<BTreeSet<Point>> = lines
        .iter()
        .map(|l| {
            if l.horizontal {
                [
                    Point::new(l.start, l.coordinate),
                    Point::new(l.end, l.coordinate),
                ]
                .into_iter()
                .collect()
            } else {
                [
                    Point::new(l.coordinate, l.start),
                    Point::new(l.coordinate, l.end),
                ]
                .into_iter()
                .collect()
            }
        })
        .collect();
    let mut count = vertices.iter().map(BTreeSet::len).sum::<usize>();
    for (i, h) in lines.iter().enumerate().filter(|(_, l)| l.horizontal) {
        for (j, v) in lines.iter().enumerate().filter(|(_, l)| !l.horizontal) {
            if h.start - v.thickness / 2.0 - 0.002 <= v.coordinate
                && v.coordinate <= h.end + v.thickness / 2.0 + 0.002
                && v.start - h.thickness / 2.0 - 0.002 <= h.coordinate
                && h.coordinate <= v.end + h.thickness / 2.0 + 0.002
            {
                charge(work, 16)?;
                let p = Point::new(v.coordinate, h.coordinate);
                count += usize::from(vertices[i].insert(p)) + usize::from(vertices[j].insert(p));
                if count > MAX_POINTS {
                    return None;
                }
            }
        }
    }
    let mut all = BTreeMap::<(bool, i64), BTreeSet<Point>>::new();
    for (line, points) in lines.iter().zip(&vertices) {
        charge(work, points.len().saturating_mul(8))?;
        all.entry((line.horizontal, (line.coordinate * SCALE).round() as i64))
            .or_default()
            .extend(points);
    }
    let mut graph = BTreeMap::<Point, BTreeSet<Point>>::new();
    let mut edges = 0usize;
    for (line, points) in lines.iter().zip(&vertices) {
        let low = *points.first()?;
        let high = *points.last()?;
        let values = all.get(&(line.horizontal, (line.coordinate * SCALE).round() as i64))?;
        let mut previous = None;
        for &p in values.range(low..=high) {
            charge(work, 8)?;
            if let Some(q) = previous {
                if q != p {
                    edges += usize::from(graph.entry(q).or_default().insert(p))
                        + usize::from(graph.entry(p).or_default().insert(q));
                    if edges > MAX_EDGES {
                        return None;
                    }
                }
            }
            previous = Some(p);
        }
    }
    trace(&graph, work)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 指定した向きと位置の、幅一ポイントの自作罫線を作る。
    fn line(horizontal: bool, coordinate: f64, start: f64, end: f64) -> Line {
        Line {
            horizontal,
            coordinate,
            start,
            end,
            thickness: 1.0,
        }
    }

    /// 行と列の間に連続した境界がある、独立した格子を作る。
    fn grid(rows: usize, columns: usize) -> Vec<Line> {
        let mut lines: Vec<_> = (0..=rows)
            .map(|r| line(true, r as f64 * 20.0, 0.0, columns as f64 * 20.0))
            .collect();
        lines.extend((0..=columns).map(|c| line(false, c as f64 * 20.0, 0.0, rows as f64 * 20.0)));
        lines
    }

    /// 表の四つの閉じた区画を、外側の無限領域と区別する。
    #[test]
    fn closed_grid_has_four_distinct_faces() {
        let found = faces(&grid(2, 2), &mut 100_000).unwrap();
        assert_eq!(found.len(), 4);
        for face in found {
            assert_eq!(face.bounds.width(), 20.0);
            assert_eq!(face.bounds.height(), 20.0);
            assert!(face.polygon.len() >= 4);
        }
    }

    /// 部分的な境界で囲まれた結合セルを、重なる矩形へ置き換えない。
    #[test]
    fn t_junction_keeps_a_nonrectangular_spanning_face() {
        let lines = vec![
            line(true, 0.0, 0.0, 40.0),
            line(true, 40.0, 0.0, 40.0),
            line(false, 0.0, 0.0, 40.0),
            line(false, 40.0, 0.0, 40.0),
            line(true, 20.0, 20.0, 40.0),
            line(false, 20.0, 20.0, 40.0),
        ];
        let found = faces(&lines, &mut 100_000).unwrap();
        assert_eq!(found.len(), 2);
        let spanning = found.iter().find(|f| f.polygon.len() > 4).unwrap();
        assert!(spanning.contains(10.0, 30.0));
        assert!(spanning.contains(30.0, 10.0));
        assert!(!spanning.contains(30.0, 30.0));
        assert!(!spanning.contains(20.0, 30.0));
    }

    /// 解析を打ち切ったあとで、一部だけの面を正しい表として使用しない。
    #[test]
    fn exhausted_budget_cannot_return_partial_faces() {
        let mut work = 1;
        assert!(faces(&grid(4, 4), &mut work).is_none());
        assert_eq!(work, 0);
    }
    /// 太い線の端と別の線の材料が接していれば、中心線の短い差を接続する。
    #[test]
    fn touching_material_connects_but_a_real_gap_does_not() {
        let mut lines = vec![
            line(true, 0.0, 0.0, 20.0),
            line(true, 20.0, 0.0, 20.0),
            line(false, 0.0, 0.5, 19.5),
            line(false, 20.0, 0.5, 19.5),
        ];
        assert_eq!(faces(&lines, &mut 100_000).unwrap().len(), 1);
        lines[2].start = 0.7;
        lines[2].end = 19.3;
        lines[3] = Line {
            coordinate: 20.0,
            ..lines[2]
        };
        assert!(faces(&lines, &mut 100_000).unwrap().is_empty());
    }

    /// 非有限の線や逆転した区間を、部分的に正しい格子へ混ぜない。
    #[test]
    fn invalid_lines_abort_the_whole_optional_inference() {
        for bad in [
            Line {
                coordinate: f64::NAN,
                ..line(true, 0.0, 0.0, 20.0)
            },
            Line {
                thickness: -1.0,
                ..line(true, 0.0, 0.0, 20.0)
            },
            line(true, 0.0, 20.0, 0.0),
        ] {
            let mut input = grid(2, 2);
            input.push(bad);
            assert!(faces(&input, &mut 100_000).is_none());
        }
    }

    /// 段組みの開いた区切り線だけでは、本文をセルへ区切らない。
    #[test]
    fn open_column_rules_do_not_invent_cells() {
        let columns = [
            line(false, 0.0, 0.0, 100.0),
            line(false, 30.0, 0.0, 100.0),
            line(false, 60.0, 0.0, 100.0),
        ];
        assert!(faces(&columns, &mut 100_000).unwrap().is_empty());
    }

    proptest! {
        /// 行列数・位置・描画順・重複線が変わっても、面の数と一意な所属を保つ。
        #[test]
        fn grid_faces_are_invariant_under_permutation_and_repeated_rules(
            rows in 2usize..9,columns in 2usize..9,
            dx in 10u16..50,dy in 10u16..50,
            shift_x in -1000i32..1000,shift_y in -1000i32..1000,
        ) {
            let mut input:Vec<_>=(0..=rows).map(|r|line(true,f64::from(shift_y)+r as f64*f64::from(dy),f64::from(shift_x),f64::from(shift_x)+columns as f64*f64::from(dx))).collect();
            input.extend((0..=columns).map(|c|line(false,f64::from(shift_x)+c as f64*f64::from(dx),f64::from(shift_y),f64::from(shift_y)+rows as f64*f64::from(dy))));
            let first=faces(&input,&mut 1_000_000).unwrap();
            prop_assert_eq!(first.len(),rows*columns);
            for r in 0..rows {for c in 0..columns {
                let x=f64::from(shift_x)+(c as f64+0.5)*f64::from(dx);
                let y=f64::from(shift_y)+(r as f64+0.5)*f64::from(dy);
                prop_assert_eq!(first.iter().filter(|face|face.contains(x,y)).count(),1);
            }}
            input.reverse();input.extend(input.clone());
            let repeated=faces(&input,&mut 1_000_000).unwrap();
            let bounds=|faces:Vec<Face>|faces.into_iter().map(|f|[f.bounds.x0,f.bounds.y0,f.bounds.x1,f.bounds.y1]).collect::<Vec<_>>();
            prop_assert_eq!(bounds(first),bounds(repeated));
        }
    }
}
