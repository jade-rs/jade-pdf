//! 通常 object の借用と、展開済み object の共有所有権を同じ API で扱う。
use std::{ops::Range, rc::Rc};

/// object番号とfallibleに保存した共有本文の組。
pub(crate) type BodyEntry = (u32, Rc<Vec<u8>>);

/// 最新 xref に選択された圧縮 object 本文。展開結果は解析内で共有する。
#[derive(Clone, Default)]
pub(crate) struct CompressedObjects {
    /// 空集合は無確保にし、完成済みの番号順配列だけを共有する。
    entries: Option<Rc<Vec<BodyEntry>>>,
}

impl CompressedObjects {
    /// 全slot予約済みの配列を番号順にし、共有管理領域も予約して保存する。
    pub(crate) fn from_entries(
        budget: &crate::decode::DecodeBudget,
        mut entries: Vec<BodyEntry>,
    ) -> Option<Self> {
        if budget.is_stopped() {
            return None;
        }
        if entries.is_empty() {
            return Some(Self::default());
        }
        entries.sort_unstable_by_key(|entry| entry.0);
        entries.dedup_by_key(|entry| entry.0);
        budget.charge_metadata_workspace_bytes(
            (std::mem::size_of::<Vec<BodyEntry>>() + 2 * std::mem::size_of::<usize>()) as u64,
        )?;
        Some(Self {
            entries: Some(Rc::new(entries)),
        })
    }

    /// 本文の番号を二分探索し、保存済み本文を複製せず返す。
    pub(crate) fn get(&self, number: &u32) -> Option<&Rc<Vec<u8>>> {
        let entries = self.entries.as_ref()?;
        let position = entries.binary_search_by_key(number, |entry| entry.0).ok()?;
        Some(&entries[position].1)
    }

    /// 番号順の共有本文を追加確保なしで走査する。
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&u32, &Rc<Vec<u8>>)> {
        self.entries
            .iter()
            .flat_map(|entries| entries.iter())
            .map(|(number, body)| (number, body))
    }

    /// 失敗時や圧縮entryのない入力の空集合を判定する。
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries
            .as_ref()
            .is_none_or(|entries| entries.is_empty())
    }
}

/// 元入力の借用、または ObjStm の展開結果の一部分。
pub(crate) enum ObjectBytes<'a> {
    /// 通常 object または直接属性の本文。
    Borrowed(&'a [u8]),
    /// 圧縮 object の共有本文と、そこから選択した値の範囲。
    Shared {
        /// 展開結果の所有権。
        bytes: Rc<Vec<u8>>,
        /// 本文内の使用範囲。
        range: Range<usize>,
    },
}

impl<'a> ObjectBytes<'a> {
    /// 圧縮本文の全範囲を複製せず保持する。
    pub(crate) fn shared(bytes: Rc<Vec<u8>>) -> Self {
        let range = 0..bytes.len();
        Self::Shared { bytes, range }
    }

    /// 配列要素などの部分範囲に所有権を引き継ぐ。
    pub(crate) fn slice(self, selected: Range<usize>) -> Option<Self> {
        self.get(selected.clone())?;
        Some(match self {
            Self::Borrowed(bytes) => Self::Borrowed(&bytes[selected]),
            Self::Shared { bytes, range } => Self::Shared {
                bytes,
                range: range.start + selected.start..range.start + selected.end,
            },
        })
    }
}

impl std::ops::Deref for ObjectBytes<'_> {
    type Target = [u8];

    /// 借用と共有本文を同じ byte slice として読む。
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(bytes) => bytes,
            Self::Shared { bytes, range } => &bytes[range.clone()],
        }
    }
}
