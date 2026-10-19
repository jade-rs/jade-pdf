//! 検査済み文書内で完成 CMap を共有する、容量制限付きの索引。
use crate::{decode::DecodeBudget, text::ToUnicodeCMap};
use std::{collections::hash_map::RandomState, hash::BuildHasher};

/// 文書内の stream と文書全体の集約を保持する解析ローカル cache。
#[derive(Default)]
pub(crate) struct CMapCache {
    /// 半分以下の占有率を保つ open addressing 配列。
    slots: Vec<Option<Entry>>,
    /// 保存済み stream 数。
    len: usize,
    /// 外部入力による意図的な hash 衝突を避ける seed。
    hash: RandomState,
    /// 完成した文書全体の集約。空 map も保存する。
    pub(crate) global: Option<ToUnicodeCMap>,
}

/// 一つの有効な stream object と完成 map。
struct Entry {
    /// 検査済み xref で選択された object 番号。
    number: u32,
    /// payload を複製せず共有する完成 map。
    cmap: ToUnicodeCMap,
}

impl CMapCache {
    /// object 番号から探索を開始する slot を求める。
    fn start(&self, number: u32, capacity: usize) -> usize {
        self.hash.hash_one(number) as usize & (capacity - 1)
    }

    /// 完成 map を共有する。未登録なら None を返す。
    pub(crate) fn get(&self, number: u32) -> Option<ToUnicodeCMap> {
        if self.slots.is_empty() {
            return None;
        }
        let mut slot = self.start(number, self.slots.len());
        for _ in 0..self.slots.len() {
            let entry = self.slots[slot].as_ref()?;
            if entry.number == number {
                return Some(entry.cmap.clone());
            }
            slot = (slot + 1) & (self.slots.len() - 1);
        }
        None
    }

    /// cache 配列を予算内で拡張して完成 map を保存する。
    pub(crate) fn insert(
        &mut self,
        budget: &DecodeBudget,
        number: u32,
        cmap: ToUnicodeCMap,
    ) -> Option<()> {
        if budget.is_stopped() {
            return None;
        }
        if self.get(number).is_some() {
            return Some(());
        }
        if self.len >= self.slots.len() / 2 {
            let capacity = self.slots.len().saturating_mul(2).max(4);
            budget.charge_tounicode_storage_bytes(
                (capacity as u64).saturating_mul(std::mem::size_of::<Option<Entry>>() as u64),
            )?;
            let mut slots = budget.try_vector_capacity(capacity)?;
            slots.resize_with(capacity, || None);
            let old = std::mem::replace(&mut self.slots, slots);
            for entry in old.into_iter().flatten() {
                self.place(entry);
            }
        }
        self.place(Entry { number, cmap });
        self.len += 1;
        Some(())
    }

    /// 空き slot がある予約済み配列へ entry を移す。
    fn place(&mut self, entry: Entry) {
        let mut slot = self.start(entry.number, self.slots.len());
        while self.slots[slot].is_some() {
            slot = (slot + 1) & (self.slots.len() - 1);
        }
        self.slots[slot] = Some(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits};
    use proptest::prelude::*;

    /// 一つの cache slot の要求容量。
    const SLOT_BYTES: u64 = std::mem::size_of::<Option<Entry>>() as u64;

    /// cache 配列だけの保存予算を作る。
    fn budget(limit: u64) -> DecodeBudget {
        DecodeBudget::new(&ParseLimits {
            max_total_tounicode_storage_bytes: limit,
            ..Default::default()
        })
    }

    /// 初回容量の境界は inclusive とし、超過した配列は作らない。
    #[test]
    fn capacity_is_checked_before_allocation() {
        for limit in [4 * SLOT_BYTES - 1, 4 * SLOT_BYTES] {
            let budget = budget(limit);
            let mut cache = CMapCache::default();
            let result = cache.insert(&budget, u32::MAX, ToUnicodeCMap::default());
            assert_eq!(result.is_some(), limit == 4 * SLOT_BYTES);
            assert_eq!(cache.get(u32::MAX).is_some(), result.is_some());
            if result.is_none() {
                assert!(cache.slots.is_empty());
                assert!(
                    matches!(budget.check(), Err(ParseError::ResourceLimit {resource: "ToUnicode storage bytes", actual, ..}) if actual == 4 * SLOT_BYTES)
                );
            }
        }
    }

    /// 再確保は旧配列との共存容量を累積加算し、重複は追加予約しない。
    #[test]
    fn growth_is_cumulative_and_duplicates_are_free() {
        let budget = budget(4 * SLOT_BYTES);
        let mut cache = CMapCache::default();
        for number in [0, u32::MAX, 0, u32::MAX] {
            cache
                .insert(&budget, number, ToUnicodeCMap::default())
                .unwrap();
        }
        assert!(cache.insert(&budget, 1, ToUnicodeCMap::default()).is_none());
        assert!(
            matches!(budget.check(), Err(ParseError::ResourceLimit {resource: "ToUnicode storage bytes", actual, ..}) if actual == 12 * SLOT_BYTES)
        );
        assert!(cache.get(0).is_some());
        assert!(cache.get(u32::MAX).is_some());
        assert!(cache.get(1).is_none());
        assert!(cache.insert(&budget, 0, ToUnicodeCMap::default()).is_none());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 衝突・成長・重複を含む lookup は、最初の完成 map を残す逐次モデルと一致する。
        #[test]
        fn lookup_matches_first_assignment_model(operations in prop::collection::vec((prop_oneof![0u32..32, any::<u32>()], any::<bool>()), 0..200)) {
            let maps = [
                crate::text::parse_tounicode_body(&DecodeBudget::default(), b"1 beginbfchar\n<01> <0041>\nendbfchar"),
                crate::text::parse_tounicode_body(&DecodeBudget::default(), b"1 beginbfchar\n<01> <0042>\nendbfchar"),
            ];
            let budget = DecodeBudget::default();
            let mut cache = CMapCache::default();
            let mut model = std::collections::BTreeMap::new();
            for (number, second) in operations {
                let value = usize::from(second);
                cache.insert(&budget, number, maps[value].clone()).unwrap();
                model.entry(number).or_insert(value);
                for (&key, &expected) in &model {
                    prop_assert!(cache.get(key).unwrap().shares_storage_with(&maps[expected]));
                }
                let absent = number.wrapping_add(1);
                prop_assert_eq!(cache.get(absent).is_some(), model.contains_key(&absent));
            }
            prop_assert!(budget.check().is_ok());
        }
    }
}
