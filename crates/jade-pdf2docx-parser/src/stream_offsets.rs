//! 抽出側の番号・offset対応を、共有metadata予算付き配列へ保存する。
use super::{DecodeBudget, xref_offset_entries};
use std::cmp::Reverse;

/// 番号・header offset・入力順を保持し、番号重複は後出しを選ぶrecord。
type Record = (u32, usize, usize);

/// 保存前の容量を予約し、番号順の二分探索でstreamを参照する索引。
#[derive(Default)]
pub(crate) struct StreamOffsets {
    /// 重複除去前の全件を予約した、番号順のrecord配列。
    entries: Vec<Record>,
}

impl StreamOffsets {
    /// 停止した予算では復元せず、検査済みstreamの全件容量を確保前に予約する。
    pub(crate) fn build(budget: &DecodeBudget, bytes: &[u8]) -> Self {
        if budget.is_stopped() {
            return Self::default();
        }
        if let Some(index) = budget.stream_index(bytes) {
            let count = index
                .streams(bytes)
                .filter(|stream| stream.object_number.is_some() && stream.object_offset.is_some())
                .count();
            let Some(mut entries) = budget.metadata_vector(count) else {
                return Self::default();
            };
            for stream in index.streams(bytes) {
                if let Some((number, offset)) = stream.object_number.zip(stream.object_offset) {
                    entries.push((number, offset, entries.len()));
                }
            }
            return Self::finish(entries);
        }
        Self::legacy(budget, bytes)
    }

    /// xrefの旧経路も、一時mapやtoken列を作らず共通配列へ保存する。
    pub(crate) fn legacy(budget: &DecodeBudget, bytes: &[u8]) -> Self {
        if budget.is_stopped() {
            return Self::default();
        }
        let mut entries = Vec::new();
        let mut save = |number, offset| {
            let order = entries.len();
            crate::metadata_workspace::try_push(budget, &mut entries, (number, offset, order))
        };
        if xref_offset_entries::visit_table(bytes, &mut save).is_none() {
            return Self::default();
        }
        // type 2への更新も後出し優先に含め、旧streamのoffsetを復活させない。
        let mut revision = Vec::new();
        let result = xref_offset_entries::visit_streams(budget, bytes, |number, entry| {
            let offset = match entry {
                super::XrefStreamEntry::Uncompressed { offset } => Some(offset),
                super::XrefStreamEntry::Compressed { .. } => None,
            };
            let order = revision.len();
            crate::metadata_workspace::try_push(budget, &mut revision, (number, offset, order))
        });
        if result.is_none() || budget.is_stopped() {
            return Self::default();
        }
        revision.sort_unstable_by_key(|&(number, _, order)| (number, Reverse(order)));
        revision.dedup_by_key(|entry| entry.0);
        for (number, offset, _) in revision {
            if let Some(offset) = offset
                && save(number, offset).is_none()
            {
                return Self::default();
            }
        }
        Self::finish(entries)
    }

    /// ソート作業の追加配列を作らず、後出し優先で番号を一意にする。
    fn finish(mut entries: Vec<Record>) -> Self {
        entries.sort_unstable_by_key(|&(number, _, order)| (number, Reverse(order)));
        entries.dedup_by_key(|entry| entry.0);
        Self { entries }
    }

    /// object番号を二分探索し、header offsetを借用する。
    pub(crate) fn get(&self, number: &u32) -> Option<&usize> {
        self.entries
            .binary_search_by_key(number, |entry| entry.0)
            .ok()
            .map(|position| &self.entries[position].1)
    }

    /// 展開対象streamを、重複除去済みの番号順で消費する。
    pub(crate) fn into_pairs(self) -> impl Iterator<Item = (u32, usize)> {
        self.entries
            .into_iter()
            .map(|(number, offset, _)| (number, offset))
    }
}
