//! 公開SDKのオブジェクト・矩形・原点による、字形セルの厳密な同一性。
use pdfium_render::prelude::Pdfium;
use std::collections::HashSet;
use std::ffi::c_void;

/// SDKオブジェクトと六つの有限座標からなる識別子。
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) struct Cell {
    /// ポインターと、負のゼロを揃えた座標のビット列。
    identity: [u64; 7],
}

impl Cell {
    /// 同じセルに属する複数のUnicode片を、原文字形の個数へ重複加算しない。
    pub(crate) fn from_values(object: usize, values: [f64; 6]) -> Option<Self> {
        if object == 0 || values.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut identity = [0; 7];
        identity[0] = object as u64;
        for (slot, value) in identity[1..].iter_mut().zip(values) {
            *slot = if value == 0. { 0 } else { value.to_bits() };
        }
        Some(Self { identity })
    }
    /// 座標が不明・非有限なら所有関係を推測しない。
    pub fn read(pdfium: &Pdfium, text: *mut c_void, index: i32) -> Option<Self> {
        let b = pdfium.bindings();
        let object = b.FPDFText_GetTextObject(text.cast(), index);
        if object.is_null() {
            return None;
        }
        let (mut l, mut r, mut bottom, mut top, mut x, mut y) = (0., 0., 0., 0., 0., 0.);
        if b.FPDFText_GetCharBox(text.cast(), index, &mut l, &mut r, &mut bottom, &mut top) == 0
            || b.FPDFText_GetCharOrigin(text.cast(), index, &mut x, &mut y) == 0
        {
            return None;
        }
        Self::from_values(object as usize, [l, r, bottom, top, x, y])
    }
}

/// 離れて並ぶUnicode片も同じ字形として数える、上限付きのセル一覧。
#[derive(Default)]
pub(crate) struct Inventory {
    /// 原文との一対一対応を照合する、重複のないセル。
    cells: HashSet<Cell>,
}
impl Inventory {
    /// 最大20万個とし、上限超過を部分的な所有証明として扱わない。
    pub fn insert(&mut self, cell: Cell) -> Option<bool> {
        if self.cells.contains(&cell) {
            return Some(false);
        }
        if self.cells.len() >= 200_000 {
            return None;
        }
        self.cells.insert(cell);
        Some(true)
    }
    /// 元の文字コード数と比較する、異なるセルの個数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 非有限値を拒否し、負のゼロの表記差を字形の個数へ加算しない。
    #[test]
    fn identity_rejects_nonfinite_values_and_canonicalizes_zero() {
        let mut cells = Inventory::default();
        assert_eq!(
            cells.insert(Cell::from_values(1, [0.; 6]).unwrap()),
            Some(true)
        );
        assert_eq!(
            cells.insert(Cell::from_values(1, [-0.; 6]).unwrap()),
            Some(false)
        );
        assert_eq!(cells.len(), 1);
        assert!(Cell::from_values(0, [0.; 6]).is_none());
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Cell::from_values(1, [v; 6]).is_none());
        }
    }
    /// 上限に達しても既存セルを重複計数せず、追加の所有証明は拒否する。
    #[test]
    fn inventory_budget_never_accepts_partial_coverage() {
        let mut cells = Inventory::default();
        for object in 1..=200_000 {
            assert_eq!(
                cells.insert(Cell::from_values(object, [0.; 6]).unwrap()),
                Some(true)
            );
        }
        assert_eq!(
            cells.insert(Cell::from_values(200_001, [0.; 6]).unwrap()),
            None
        );
        assert_eq!(
            cells.insert(Cell::from_values(1, [0.; 6]).unwrap()),
            Some(false)
        );
        assert_eq!(cells.len(), 200_000);
    }
}
