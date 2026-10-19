//! 文字抽出とForm訪問のページ本文・共有本体・payload cacheをmetadata容量へ予約する。
use crate::{decode::DecodeBudget, metadata_map::Map};
use std::rc::Rc;

/// 本文を複製せず、Vec本体とstrong/weak counterの要求容量を予約して共有する。
fn share(budget: &DecodeBudget, bytes: Vec<u8>) -> Option<Rc<Vec<u8>>> {
    budget.charge_metadata_workspace_bytes(
        (std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>()) as u64,
    )?;
    Some(Rc::new(bytes))
}

/// ページ順と元のstream順で連結し、途中失敗時は完成ページも含め全体を破棄する。
pub(crate) fn pages(
    budget: &DecodeBudget,
    mut streams: Vec<(usize, Vec<u8>)>,
) -> Option<Vec<(usize, Rc<Vec<u8>>)>> {
    let mut order = budget.metadata_vector(streams.len())?;
    order.extend(0..streams.len());
    order.sort_unstable_by_key(|&index| (streams[index].0, index));
    let count = order
        .iter()
        .enumerate()
        .filter(|&(index, &stream)| index == 0 || streams[order[index - 1]].0 != streams[stream].0)
        .count();
    let mut pages = budget.metadata_vector(count)?;
    let mut cursor = 0;
    while cursor < order.len() {
        let page = streams[order[cursor]].0;
        let end = cursor + order[cursor..].partition_point(|&index| streams[index].0 == page);
        let length = order[cursor..end].iter().fold(0usize, |length, &index| {
            length
                .saturating_add(streams[index].1.len())
                .saturating_add(1)
        });
        let mut joined = budget.metadata_vector(length)?;
        for &index in &order[cursor..end] {
            joined.extend_from_slice(&streams[index].1);
            joined.push(b'\n');
            // 入力の展開済み本文はコピー完了後に解放し、ピーク容量を抑える。
            streams[index].1 = Vec::new();
        }
        pages.push((page, share(budget, joined)?));
        cursor = end;
    }
    Some(pages)
}

/// 完成本文と欠落をobject番号ごとに共有し、slot容量を取得前に予約するcache。
#[derive(Default)]
pub(crate) struct Payloads {
    /// 拡張時の全slot容量を累積する、番号から共有本文への対応。
    entries: Map<u32, Option<Rc<Vec<u8>>>>,
}

impl Payloads {
    /// hitでは本文を再取得せず、停止後のhitと失敗途中の保存を拒否する。
    pub(crate) fn get_or_load(
        &mut self,
        budget: &DecodeBudget,
        number: u32,
        load: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Option<Rc<Vec<u8>>> {
        if budget.is_stopped() {
            return None;
        }
        if let Some(value) = self.entries.get(&number) {
            return value.clone();
        }
        self.entries.prepare(budget, &number)?;
        let bytes = load();
        if budget.is_stopped() {
            return None;
        }
        let shared = match bytes {
            Some(bytes) => Some(share(budget, bytes)?),
            None => None,
        };
        self.entries.insert(budget, number, shared.clone())?;
        shared
    }
}

#[cfg(test)]
#[path = "form_storage_tests.rs"]
mod tests;
