//! 表の罫線が囲む閉領域を、原文の文字を変更せず調べるための部品。
mod geometry;
mod reader;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod sdk_contract_tests;

use crate::Bounds;
use pdfium_render::prelude::Pdfium;
use std::{collections::BTreeMap, ffi::c_void};

/// 罫線から推定したセル。タグ構造による所有とは独立に保持する。
#[derive(Default, Debug)]
pub(crate) struct Plan {
    /// 閉じた罫線の面。座標は物理ポイントに正規化している。
    pub faces: Vec<geometry::Face>,
    /// 頂点を共有する面の連結成分。
    pub components: Vec<usize>,
    /// 抽出の座標を物理ポイントへ戻す除数。
    point: f64,
}

impl Plan {
    /// 原文の文字を変えず、任意推定用の共有予算の範囲で面を準備する。
    pub(crate) fn read(
        pdfium: &Pdfium,
        page: *mut c_void,
        point: f64,
        characters: usize,
        work: &mut usize,
    ) -> Self {
        Self::build(pdfium, page, point, characters, work).unwrap_or_default()
    }

    /// 上限超過時には途中の面を返さない。
    fn build(
        pdfium: &Pdfium,
        page: *mut c_void,
        point: f64,
        characters: usize,
        work: &mut usize,
    ) -> Option<Self> {
        let lines = reader::read(pdfium, page, point)?;
        let faces = geometry::faces(&lines, work)?;
        let vertices: usize = faces.iter().map(|f| f.polygon.len()).sum();
        // 一文字の四隅と中心を両パスで照合しても、保存済み全ページの予算を超えない。
        let cost = vertices.saturating_mul(characters).saturating_mul(12);
        if cost > *work {
            *work = 0;
            return None;
        }
        *work -= cost;
        let mut components: Vec<_> = (0..faces.len()).collect();
        let mut owners = BTreeMap::new();
        for (i, face) in faces.iter().enumerate() {
            for &(x, y) in &face.polygon {
                let key = ((x * 4096.0).round() as i64, (y * 4096.0).round() as i64);
                if let Some(&other) = owners.get(&key) {
                    let a = root(&mut components, i);
                    let b = root(&mut components, other);
                    components[a] = b;
                } else {
                    owners.insert(key, i);
                }
            }
        }
        for i in 0..components.len() {
            components[i] = root(&mut components, i);
        }
        Some(Self {
            faces,
            components,
            point,
        })
    }

    /// 字面全体が一つの面の内部にある場合だけ、その面の識別子を返す。
    pub(crate) fn owner(&self, b: Bounds) -> Option<usize> {
        let (x0, x1, y0, y1) = (
            b.x0 / self.point,
            b.x1 / self.point,
            b.y0 / self.point,
            b.y1 / self.point,
        );
        let mut found = None;
        for (index, face) in self.faces.iter().enumerate() {
            if [(x0,y0), (x0,y1), (x1,y0), (x1,y1), ((x0+x1)/2.0, (y0+y1)/2.0)]
                .into_iter().all(|(x,y)| face.contains(x,y))
                // 頂点が字面の外にあっても、長い壁が途中を通り抜けることがある。
                // 閉矩形と境界の各線分を照合し、切り欠きや内部の壁を埋めない。
                && !face.polygon.iter().zip(face.polygon.iter().cycle().skip(1))
                    .take(face.polygon.len()).any(|(&(ax,ay),&(bx,by))| {
                        if ax == bx {
                            x0 <= ax && ax <= x1 && ay.min(by) <= y1 && y0 <= ay.max(by)
                        } else if ay == by {
                            y0 <= ay && ay <= y1 && ax.min(bx) <= x1 && x0 <= ax.max(bx)
                        } else { true }
                    })
            {
                if found.is_some() {
                    return None;
                }
                found = Some(index);
            }
        }
        found
    }
}

/// 頂点共有の親をたどる。木は既に確認済みの有限な面だけから成る。
fn root(parents: &mut [usize], mut index: usize) -> usize {
    let mut top = index;
    while parents[top] != top {
        top = parents[top];
    }
    while parents[index] != index {
        let next = parents[index];
        parents[index] = top;
        index = next;
    }
    top
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    /// 四隅と中心が内部でも、途中に別セルへの細い切り欠きがあれば所属を推定しない。
    #[test]
    fn owned_rectangle_does_not_cross_an_off_center_notch() {
        for scale in [0.5, 1.0, 8.0] {
            for shift in [-1000.0, 0.0, 1000.0] {
                let bounds = |x0, x1, y0, y1| Bounds {
                    x0: x0 * scale + shift,
                    x1: x1 * scale + shift,
                    y0: y0 * scale + shift / 2.0,
                    y1: y1 * scale + shift / 2.0,
                };
                let polygon = [
                    (0., 0.),
                    (10., 0.),
                    (10., 10.),
                    (8., 10.),
                    (8., 2.),
                    (7., 2.),
                    (7., 10.),
                    (0., 10.),
                ]
                .into_iter()
                .map(|(x, y)| (x * scale + shift, y * scale + shift / 2.0))
                .collect();
                let plan = Plan {
                    faces: vec![geometry::Face {
                        polygon,
                        bounds: bounds(0., 10., 0., 10.),
                    }],
                    components: vec![0],
                    point: 1.0,
                };
                let crossing = bounds(1., 9., 4., 5.);
                for (x, y) in [
                    (crossing.x0, crossing.y0),
                    (crossing.x0, crossing.y1),
                    (crossing.x1, crossing.y0),
                    (crossing.x1, crossing.y1),
                    crossing.center(),
                ] {
                    assert!(
                        plan.faces[0].contains(x, y),
                        "The control's corners and center must lie inside"
                    );
                }
                assert_eq!(plan.owner(crossing), None, "scale={scale}, shift={shift}");
                assert_eq!(plan.owner(bounds(1., 2., 4., 5.)), Some(0));
                assert_eq!(plan.owner(bounds(8.5, 9., 4., 5.)), Some(0));
            }
        }
    }
}
