//! 矩形照会のたびに全glyphを走査せず、包含範囲の一様なweightをまとめて確認する。
use super::Glyph;
use crate::{ParseError, decode::DecodeBudget, metadata_workspace};

/// 非空の範囲の照合結果。交差なしと未確認を混同しない。
#[derive(Clone, Copy)]
enum Hit {
    Empty,
    Uniform(u16),
    Unknown,
}

/// glyph範囲を半分ずつ分けた包絡矩形と一様値。
struct Node {
    /// 子孫全glyphの左・下・右・上。
    bounds: [f32; 4],
    /// 子孫全glyphが同じ確認値の場合だけ保持する。
    uniform: Option<u16>,
    /// 葉のglyph範囲。内部節点も非空の範囲を保持する。
    range: std::ops::Range<usize>,
    /// 平衡な左右の子。葉はNone。
    children: Option<(usize, usize)>,
}

/// 並べ替えた観察値を参照する、予算付きの平衡索引。
pub(super) struct Tree {
    /// index0を根とする包絡矩形配列。
    nodes: Vec<Node>,
    /// テストだけで照会の訪問数を観察する。
    #[cfg(test)]
    pub(super) visited: std::cell::Cell<usize>,
}

impl Tree {
    /// SDK観察値だけを並べ替え、本文spanの順序を変更しない。
    pub(super) fn build(budget: &DecodeBudget, glyphs: &mut [Glyph]) -> Result<Self, ParseError> {
        metadata_workspace::ensure_active(budget)?;
        glyphs.sort_unstable_by(|a, b| a.bounds[0].total_cmp(&b.bounds[0]));
        let mut tree = Self {
            nodes: Vec::new(),
            #[cfg(test)]
            visited: std::cell::Cell::new(0),
        };
        if !glyphs.is_empty() {
            tree.branch(budget, glyphs, 0..glyphs.len())?;
        }
        metadata_workspace::ensure_active(budget)?;
        Ok(tree)
    }

    /// 有限の非空範囲を二分し、全要求容量を共有metadata予算へ計上する。
    fn branch(
        &mut self,
        budget: &DecodeBudget,
        glyphs: &[Glyph],
        range: std::ops::Range<usize>,
    ) -> Result<usize, ParseError> {
        metadata_workspace::ensure_active(budget)?;
        let first = &glyphs[range.start];
        let mut bounds = first.bounds;
        let mut uniform = first.weight;
        for (index, glyph) in glyphs[range.clone()].iter().enumerate() {
            if index % 256 == 0 {
                metadata_workspace::ensure_active(budget)?;
            }
            bounds[0] = bounds[0].min(glyph.bounds[0]);
            bounds[1] = bounds[1].min(glyph.bounds[1]);
            bounds[2] = bounds[2].max(glyph.bounds[2]);
            bounds[3] = bounds[3].max(glyph.bounds[3]);
            if uniform != glyph.weight {
                uniform = None;
            }
        }
        let position = self.nodes.len();
        metadata_workspace::push(
            budget,
            &mut self.nodes,
            Node {
                bounds,
                uniform,
                range: range.clone(),
                children: None,
            },
        )?;
        if range.len() > 8 {
            let middle = range.start + range.len() / 2;
            let left = self.branch(budget, glyphs, range.start..middle)?;
            let right = self.branch(budget, glyphs, middle..range.end)?;
            self.nodes[position].children = Some((left, right));
        }
        Ok(position)
    }

    /// 確認値だけを公開し、混在・未知・交差なしは未確認へ戻す。
    pub(super) fn weight(
        &self,
        budget: &DecodeBudget,
        glyphs: &[Glyph],
        rect: [f32; 4],
    ) -> Result<Option<u16>, ParseError> {
        metadata_workspace::ensure_active(budget)?;
        if self.nodes.is_empty() {
            return Ok(None);
        }
        Ok(match self.query(budget, glyphs, rect, 0)? {
            Hit::Uniform(value) => Some(value),
            _ => None,
        })
    }

    /// 完全包含なら集約値だけを読み、部分交差の枝だけを降りる。
    fn query(
        &self,
        budget: &DecodeBudget,
        glyphs: &[Glyph],
        rect: [f32; 4],
        position: usize,
    ) -> Result<Hit, ParseError> {
        metadata_workspace::ensure_active(budget)?;
        #[cfg(test)]
        self.visited.set(self.visited.get() + 1);
        let node = &self.nodes[position];
        let b = node.bounds;
        if rect[0] > b[2] || rect[2] < b[0] || rect[1] > b[3] || rect[3] < b[1] {
            return Ok(Hit::Empty);
        }
        if rect[0] <= b[0] && rect[1] <= b[1] && rect[2] >= b[2] && rect[3] >= b[3] {
            return Ok(node.uniform.map_or(Hit::Unknown, Hit::Uniform));
        }
        if let Some((left, right)) = node.children {
            let first = self.query(budget, glyphs, rect, left)?;
            if matches!(first, Hit::Unknown) {
                return Ok(first);
            }
            return Ok(merge(first, self.query(budget, glyphs, rect, right)?));
        }
        let mut hit = Hit::Empty;
        for glyph in &glyphs[node.range.clone()] {
            let b = glyph.bounds;
            let x = rect[0].max(b[0]);
            let y = rect[1].max(b[1]);
            let right = rect[2].min(b[2]);
            let top = rect[3].min(b[3]);
            if (x < right && y < top) || ((b[0] == b[2] || b[1] == b[3]) && x <= right && y <= top)
            {
                hit = merge(hit, glyph.weight.map_or(Hit::Unknown, Hit::Uniform));
                if matches!(hit, Hit::Unknown) {
                    break;
                }
            }
        }
        Ok(hit)
    }
}

/// 未確認を優先し、同値の確認だけを結合する。
fn merge(a: Hit, b: Hit) -> Hit {
    match (a, b) {
        (Hit::Empty, value) | (value, Hit::Empty) => value,
        (Hit::Uniform(a), Hit::Uniform(b)) if a == b => Hit::Uniform(a),
        _ => Hit::Unknown,
    }
}
