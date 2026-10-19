//! xrefの一時entry・履歴・旧offsetを、共有metadata予算付き配列へ保存する。
use super::Entry;
use crate::{ParseError, decode::DecodeBudget, metadata_workspace};
use std::cmp::Reverse;

/// 重複除去前のentryと、同じrevision内の優先順位。
pub(super) struct StagedEntry {
    /// object番号。
    number: usize,
    /// hybrid補助streamをtableより優先する印。
    supplemental: bool,
    /// 同じ入力元では先に記載されたentryを選ぶ順番。
    order: usize,
    /// 通常・圧縮objectまたは削除。
    entry: Entry,
}

/// revision内の全entryを確保前に予約し、重複は追加配列なしで除く。
#[derive(Default)]
pub(super) struct RevisionEntries {
    /// 初回と拡張時の全要求容量を予約した配列。
    entries: Vec<StagedEntry>,
}

impl RevisionEntries {
    /// 同じ番号の重複も保存前に容量へ数え、補助streamの優先順位を保持する。
    pub(super) fn insert(
        &mut self,
        budget: &DecodeBudget,
        number: usize,
        entry: Entry,
        supplemental: bool,
    ) -> Result<(), ParseError> {
        let order = self.entries.len();
        metadata_workspace::push(
            budget,
            &mut self.entries,
            StagedEntry {
                number,
                supplemental,
                order,
                entry,
            },
        )
    }

    /// 補助streamを優先し、各入力元の最初のentryを番号順で消費する。
    pub(super) fn into_entries(mut self) -> impl Iterator<Item = (usize, Entry)> {
        self.entries
            .sort_unstable_by_key(|entry| (entry.number, Reverse(entry.supplemental), entry.order));
        self.entries.dedup_by_key(|entry| entry.number);
        self.entries
            .into_iter()
            .map(|entry| (entry.number, entry.entry))
    }
}

/// 番号順・revision昇順で保存する、一つの有効entry。
pub(super) struct RevisionEntry {
    /// object番号。
    number: usize,
    /// 最新を0とし、Prevを辿るたびに増えるrevision番号。
    revision: usize,
    /// 通常・圧縮objectまたは削除。
    entry: Entry,
}

/// 番号・世代・旧offset・最後に有効だったrevision。
pub(super) type HistoricalOffset = (usize, usize, usize, usize);

/// 正式xrefの履歴と旧stream参照時点を、二分探索できる配列へ保存する。
#[derive(Default)]
pub(super) struct History {
    /// 番号順・revision昇順の全履歴。
    entries: Vec<RevisionEntry>,
    /// 番号・世代・旧offset順で重複を除いた対応。
    offsets: Vec<HistoricalOffset>,
}

impl History {
    /// 全履歴の配列を拡張前に予約し、解放や再作成も累積へ数える。
    pub(super) fn push(
        &mut self,
        budget: &DecodeBudget,
        number: usize,
        revision: usize,
        entry: Entry,
    ) -> Result<(), ParseError> {
        metadata_workspace::push(
            budget,
            &mut self.entries,
            RevisionEntry {
                number,
                revision,
                entry,
            },
        )
    }

    /// 旧offsetの重複除去前の件数を予約し、追加作業配列なしで索引を完成する。
    pub(super) fn finish(mut self, budget: &DecodeBudget) -> Result<Self, ParseError> {
        self.entries
            .sort_unstable_by_key(|entry| (entry.number, entry.revision));
        let old_offset = |pair: &[RevisionEntry]| match &pair[1].entry {
            Entry::Uncompressed { generation, offset } if pair[0].number == pair[1].number => {
                Some((pair[1].number, *generation, *offset, pair[0].revision + 1))
            }
            _ => None,
        };
        let count = self.entries.windows(2).filter_map(old_offset).count();
        let mut offsets = metadata_workspace::vector(budget, count)?;
        offsets.extend(self.entries.windows(2).filter_map(old_offset));
        offsets.sort_unstable();
        offsets.dedup_by_key(|entry| (entry.0, entry.1, entry.2));
        self.offsets = offsets;
        Ok(self)
    }

    /// 指定revisionに有効だったentryを、番号とrevisionの二分探索で返す。
    pub(super) fn entry_at(&self, number: usize, revision: usize) -> Option<&Entry> {
        let position = self
            .entries
            .partition_point(|entry| (entry.number, entry.revision) < (number, revision));
        self.entries
            .get(position)
            .filter(|entry| entry.number == number)
            .map(|entry| &entry.entry)
    }

    /// 各番号の最新entryだけから、所属streamとmember indexを借用列挙する。
    pub(super) fn compressed(&self) -> impl Iterator<Item = (u32, u32, u32)> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(position, record)| {
                if position > 0 && self.entries[position - 1].number == record.number {
                    return None;
                }
                match record.entry {
                    Entry::Compressed {
                        object_stream,
                        index,
                    } => Some((
                        u32::try_from(record.number).ok()?,
                        u32::try_from(object_stream).ok()?,
                        u32::try_from(index).ok()?,
                    )),
                    _ => None,
                }
            })
    }

    /// 置換済みstreamが最後に有効だった時点を、物理offsetの二分探索で返す。
    pub(super) fn stream_revision(&self, number: u32, generation: usize, offset: usize) -> usize {
        let key = (number as usize, generation, offset);
        self.offsets
            .binary_search_by_key(&key, |entry| (entry.0, entry.1, entry.2))
            .ok()
            .map(|position| self.offsets[position].3)
            .unwrap_or(0)
    }
}
