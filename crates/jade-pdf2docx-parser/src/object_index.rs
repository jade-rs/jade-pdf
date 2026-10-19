//! metadata の有効 object 索引を容量制限付きの借用配列で保存する。
use crate::{decode::DecodeBudget, metadata};
use std::cmp::Reverse;

/// generation と入力を借用した本文を object 番号順に保存する索引。
#[derive(Default)]
pub(crate) struct Objects<'a> {
    /// 確保前に予算を予約した配列。重複除去後も元の容量を保持する。
    entries: Vec<Entry<'a>>,
}

/// 同番号の後勝ち順序と本文を保持する一件。
struct Entry<'a> {
    /// 最新 xref の有効 object 番号。
    number: u32,
    /// object 世代とコピーしない本文。
    value: (u16, &'a [u8]),
    /// 同じ番号で最後に出現する本文を選ぶ走査位置。
    order: usize,
}

impl<'a> Objects<'a> {
    /// 二分探索で世代・本文を借用する。
    pub(crate) fn get(&self, number: &u32) -> Option<&(u16, &'a [u8])> {
        let position = self
            .entries
            .binary_search_by_key(number, |entry| entry.number)
            .ok()?;
        Some(&self.entries[position].value)
    }

    /// object 番号順にコピーせず走査する。
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&u32, &(u16, &'a [u8]))> {
        self.entries
            .iter()
            .map(|entry| (&entry.number, &entry.value))
    }

    /// allocation を行わない空の fixture 索引を作る。
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// fixture の重複除去後の件数を返す。
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// allocation を伴わない sort と重複除去で BTreeMap の後勝ちを保つ。
    fn finish(&mut self) {
        self.entries
            .sort_unstable_by_key(|entry| (entry.number, Reverse(entry.order)));
        self.entries.dedup_by_key(|entry| entry.number);
    }
}

/// 件数を先に数え、一つの fallible 配列へ本文を借用して保存する。
pub(crate) fn build<'a>(budget: &DecodeBudget, source: &'a [u8]) -> Option<Objects<'a>> {
    if budget.is_stopped() {
        return None;
    }
    let count = metadata::objects(source, None).count();
    let mut entries = budget.metadata_vector(count)?;
    for (order, object) in metadata::objects(source, None).enumerate() {
        entries.push(Entry {
            number: object.number,
            value: (object.generation, object.body),
            order,
        });
    }
    let mut objects = Objects { entries };
    objects.finish();
    Some(objects)
}

#[cfg(test)]
impl<'a> FromIterator<(u32, (u16, &'a [u8]))> for Objects<'a> {
    /// テスト用の本文から fixture 索引を作る。production は build の予算を使う。
    fn from_iter<T: IntoIterator<Item = (u32, (u16, &'a [u8]))>>(values: T) -> Self {
        let entries = values
            .into_iter()
            .enumerate()
            .map(|(order, (number, value))| Entry {
                number,
                value,
                order,
            })
            .collect();
        let mut objects = Self { entries };
        objects.finish();
        objects
    }
}

#[cfg(test)]
impl<'a, const N: usize> From<[(u32, (u16, &'a [u8])); N]> for Objects<'a> {
    /// 小さな配列から既存のテスト fixture を構築する。
    fn from(values: [(u32, (u16, &'a [u8])); N]) -> Self {
        values.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits};
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    /// object 索引だけの要求容量予算を作る。
    fn budget(limit: u64) -> DecodeBudget {
        DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: limit,
            ..Default::default()
        })
    }

    /// 本文を借用する一 entry の要求容量。
    const ENTRY_BYTES: u64 = std::mem::size_of::<Entry<'static>>() as u64;

    /// 同じ番号の最後の世代・本文を保存し、object 本文は入力を借用する。
    #[test]
    fn keeps_last_generation_and_borrows_body() {
        let source = b"9 0 obj /Old endobj\n1 0 obj /First endobj\n9 2 obj /New endobj\n";
        let objects = build(&DecodeBudget::default(), source).unwrap();
        let &(generation, body) = objects.get(&9).unwrap();
        assert_eq!(generation, 2);
        assert_eq!(body, b" /New ");
        assert!(body.as_ptr() >= source.as_ptr());
        assert!(body.as_ptr() as usize + body.len() <= source.as_ptr() as usize + source.len());
        assert!(objects.get(&8).is_none());
    }

    /// 非空の索引は予算0で構築せず、公開資源エラーを保持する。
    #[test]
    fn zero_budget_rejects_object_index() {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: 0,
            ..Default::default()
        });
        assert!(build(&budget, b"1 0 obj /Value endobj\n").is_none());
        assert!(matches!(budget.check(), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", limit: 0, actual
        }) if actual > 0));
    }

    /// 後勝ちで除去した entry と再作成も数え、inclusive な境界を守る。
    #[test]
    fn cumulative_capacity_counts_duplicates_and_rebuilding() {
        let source = b"1 0 obj /Old endobj\n1 2 obj /New endobj\n2 0 obj /Other endobj\n";
        let capacity = 3 * ENTRY_BYTES;
        let limited = budget(capacity - 1);
        assert!(build(&limited, source).is_none());
        assert!(matches!(limited.check(), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", limit, actual
        }) if limit == capacity - 1 && actual == capacity));
        let exact = budget(capacity);
        let objects = build(&exact, source).unwrap();
        assert_eq!(objects.len(), 2);
        assert_eq!(objects.get(&1), Some(&(2, b" /New ".as_slice())));
        assert!(build(&exact, source).is_none());
        assert!(matches!(exact.check(), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", limit, actual
        }) if limit == capacity && actual == 2 * capacity));
        assert!(build(&exact, b"").is_none(), "エラー取得後も停止を維持する");
        assert!(
            build(&budget(capacity), source).is_some(),
            "解析間は独立する"
        );
    }

    /// object のない入力は保存配列を確保せず、予算0でも空の索引を返す。
    #[test]
    fn empty_index_uses_no_capacity() {
        let budget = budget(0);
        for source in [
            b"".as_slice(),
            b"% 1 0 obj /False endobj\n",
            b"(2 0 obj /False endobj)",
        ] {
            assert_eq!(build(&budget, source).unwrap().len(), 0);
        }
        assert!(budget.check().is_ok());
    }

    /// fallible な metadata 配列の確保失敗を保持し、後続の構築も停止する。
    #[test]
    fn allocation_failure_stops_index_building() {
        let budget = budget(u64::MAX);
        assert!(budget.metadata_vector::<u8>(usize::MAX).is_none());
        assert!(
            matches!(budget.check(), Err(ParseError::Io(error)) if error.kind() == std::io::ErrorKind::OutOfMemory)
        );
        assert!(build(&budget, b"1 0 obj /Value endobj\n").is_none());
        assert!(build(&budget, b"").is_none());
    }

    /// 索引超過後は Root のない互換入力の Page 列挙も再開しない。
    #[test]
    fn stopped_metadata_budget_prevents_legacy_page_enumeration() {
        let budget = budget(0);
        assert!(build(&budget, b"1 0 obj /Value endobj\n").is_none());
        assert!(budget.check().is_err());
        assert!(
            budget
                .page_dictionaries(b"<< /Type /Page /MediaBox [0 0 10 10] >>")
                .is_empty()
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        /// 独立した BTreeMap と番号順・後勝ち世代・本文・借用範囲を比較する。
        #[test]
        fn sorted_index_matches_independent_map(values in prop::collection::vec((any::<u32>(), any::<u16>()), 0..128)) {
            let mut source = String::new();
            let mut expected = BTreeMap::new();
            for (order, (number, generation)) in values.into_iter().enumerate() {
                source.push_str(&format!("{number} {generation} obj /Value{order} endobj\n"));
                expected.insert(number, (generation, format!(" /Value{order} ")));
            }
            let objects = build(&DecodeBudget::default(), source.as_bytes()).unwrap();
            prop_assert_eq!(objects.len(), expected.len());
            for ((&number, &(generation, body)), (expected_number, (expected_generation, expected_body))) in objects.iter().zip(expected) {
                prop_assert_eq!(number, expected_number);
                prop_assert_eq!(generation, expected_generation);
                prop_assert_eq!(body, expected_body.as_bytes());
                prop_assert!(body.as_ptr() >= source.as_ptr());
                prop_assert!(body.as_ptr() as usize + body.len() <= source.as_ptr() as usize + source.len());
                prop_assert_eq!(objects.get(&number), Some(&(generation, body)));
            }
        }
    }
}
