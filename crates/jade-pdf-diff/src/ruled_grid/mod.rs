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
                // 凹んだセルの壁を長い字面がまたぐ場合も、四隅だけで受け入れない。
                && !face.polygon.iter().any(|&(x,y)| x0 < x && x < x1 && y0 < y && y < y1)
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
