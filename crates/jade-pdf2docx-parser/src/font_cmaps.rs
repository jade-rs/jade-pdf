//! Font objectのCMap索引を容量制限付き番号順配列へ保存する。
use super::{DecodeBudget, StreamOffsets, ToUnicodeCMap};

/// 完成CMapの共有所有者を保存する索引。
#[derive(Default)]
pub(super) struct Index {
    /// 全object候補slotを予約した番号/世代/CMapの番号順配列。
    entries: Vec<(u32, u16, ToUnicodeCMap)>,
}
impl Index {
    /// CMapを複製せず番号で借用する。
    pub(super) fn get(&self, number: &u32) -> Option<&ToUnicodeCMap> {
        let position = self
            .entries
            .binary_search_by_key(number, |entry| entry.0)
            .ok()?;
        Some(&self.entries[position].2)
    }
    /// 保存したCMapがないことを検査する。
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
impl IntoIterator for Index {
    type Item = (u32, u16, ToUnicodeCMap);
    type IntoIter = std::vec::IntoIter<Self::Item>;
    /// fallbackを番号順へ走査し、世代と共有CMapを移す。
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

/// 全候補slotを作成前に予約し、解析途中の停止時は部分索引を捨てる。
pub(super) fn read(
    budget: &DecodeBudget,
    bytes: &[u8],
    objects: &crate::page_attributes::Objects<'_>,
    offsets: &StreamOffsets,
) -> Index {
    let Some(mut entries) = budget.metadata_vector(objects.iter().count()) else {
        return Index::default();
    };
    let mut resolver = crate::resource_resolver::ResourceResolver::new(objects, budget);
    for (&number, (generation, dictionary)) in objects.iter() {
        if super::is_font_dictionary(&mut resolver, dictionary)
            && let Some(stream) = reference(objects, dictionary)
            && let Some(cmap) = super::font_cmap_for_object_number(budget, bytes, stream, offsets)
            && !cmap.is_empty()
        {
            entries.push((number, *generation, cmap));
        }
        if budget.is_stopped() {
            return Index::default();
        }
    }
    Index { entries }
}

/// ToUnicode参照を借用解析し、存在する最新objectと世代が一致する場合だけ返す。
pub(super) fn reference(
    objects: &crate::page_attributes::Objects<'_>,
    dictionary: &[u8],
) -> Option<u32> {
    let (number, generation) = crate::metadata::value(dictionary, b"ToUnicode")
        .and_then(crate::metadata::reference_with_generation)?;
    let &(actual, _) = objects.get(&number)?;
    (actual == generation).then_some(number)
}
