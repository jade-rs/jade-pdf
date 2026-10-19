//! FontのBaseFont・Encodingを番号順の容量制限付き配列へ保存する。
use crate::{decode::DecodeBudget, page_attributes::Objects, resource_resolver::ResourceResolver};

/// object番号順のFont名索引。文字列とslotの全確保をmetadata予算に含める。
#[derive(Default)]
pub(crate) struct Names {
    /// 全object件数を予約した番号/世代/名前の配列。非Fontや無効名のslotも数える。
    entries: Vec<(u32, u16, String)>,
}

impl Names {
    /// 保存済み名を二分探索し、文字列を複製せず借用する。
    pub(crate) fn get(&self, number: &u32) -> Option<&String> {
        let position = self
            .entries
            .binary_search_by_key(number, |entry| entry.0)
            .ok()?;
        Some(&self.entries[position].2)
    }
}

impl IntoIterator for Names {
    type Item = (u32, u16, String);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    /// object番号順で世代と名前をコピーせず移し、staleなfallback参照を除外する。
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

/// 既知object件数のslotを予約し、有効Font名の保存失敗時は部分索引を捨てる。
pub(super) fn read(budget: &DecodeBudget, objects: &Objects<'_>, key: &[u8]) -> Names {
    let Some(mut entries) = budget.metadata_vector(objects.iter().count()) else {
        return Names::default();
    };
    let mut resolver = ResourceResolver::new(objects, budget);
    for (&number, (generation, dictionary)) in objects.iter() {
        if super::is_font_dictionary(&mut resolver, dictionary)
            && let Some(name) = crate::metadata_names::name(budget, dictionary, key)
        {
            entries.push((number, *generation, name));
        }
        if budget.is_stopped() {
            return Names::default();
        }
    }
    Names { entries }
}
