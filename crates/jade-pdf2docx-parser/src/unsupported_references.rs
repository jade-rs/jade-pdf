//! 未対応描画のresource参照を既存名の借用索引で検索する。
use crate::{decode::DecodeBudget, metadata_names};
use std::cmp::Ordering;

/// 名前を複製せず、ページ/復号名と元の順序で整列した参照索引。
pub(super) struct References {
    /// 抽出時に予算へ予約された名前と参照。
    values: Vec<(usize, String, u32)>,
    /// 重複aliasの最後の値も保持する、予約済みの元位置配列。
    order: Vec<usize>,
}

impl References {
    /// 索引の全候補slotを確保前に予約し、名前を複製せず整列する。
    pub(super) fn new(budget: &DecodeBudget, values: Vec<(usize, String, u32)>) -> Option<Self> {
        let mut order = budget.metadata_vector(values.len())?;
        order.extend(0..values.len());
        order.sort_unstable_by(|&left, &right| {
            let a = &values[left];
            let b = &values[right];
            (a.0, &a.1, left).cmp(&(b.0, &b.1, right))
        });
        Some(Self { values, order })
    }

    /// escape/非UTF-8 byteを借用比較し、同じaliasでは最後の参照を返す。
    pub(super) fn get(&self, page: usize, raw: &[u8]) -> Option<u32> {
        if !metadata_names::valid_resource_name(raw) {
            return None;
        }
        let compare = |index: usize| {
            let (candidate, name, _) = &self.values[index];
            candidate
                .cmp(&page)
                .then_with(|| metadata_names::resource_name_order(raw, name).reverse())
        };
        let end = self
            .order
            .partition_point(|&index| compare(index) != Ordering::Greater);
        let &index = self.order.get(end.checked_sub(1)?)?;
        (compare(index) == Ordering::Equal).then_some(self.values[index].2)
    }
}

#[cfg(test)]
#[path = "unsupported_references_tests.rs"]
mod tests;
