//! Resource/Font の共有索引を fallible な配列と累積予算で管理する。
use crate::decode::DecodeBudget;
use std::{collections::hash_map::RandomState, hash::BuildHasher};

/// object 番号をキーにし、占有率を半分以下に保つ容量制限付き cache。
pub(crate) struct Cache<T> {
    /// 予算検査後に確保する open addressing 配列。
    slots: Vec<Option<Entry<T>>>,
    /// 保存済み entry 数。
    len: usize,
    /// 未信頼 object 番号の意図的な hash 衝突を避ける seed。
    hash: RandomState,
}

/// 一つの有効 object と共有値。
struct Entry<T> {
    /// 不変 object index 内の番号。
    number: u32,
    /// 参照結果または共有 Font 本体。
    value: T,
}

impl<T> Default for Cache<T> {
    /// 最初の参照まで allocation を行わない空 cache を作る。
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            len: 0,
            hash: RandomState::new(),
        }
    }
}

impl<T> Cache<T> {
    /// power of two の配列内で探索開始位置を求める。
    fn start(&self, number: u32) -> usize {
        self.hash.hash_one(number) as usize & (self.slots.len() - 1)
    }

    /// 未登録値と循環探索を有限回の探索で区別する。
    fn position(&self, number: u32) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        let mut slot = self.start(number);
        for _ in 0..self.slots.len() {
            let entry = self.slots[slot].as_ref()?;
            if entry.number == number {
                return Some(slot);
            }
            slot = (slot + 1) & (self.slots.len() - 1);
        }
        None
    }

    /// 登録済み値を payload の複製なしで借用する。
    pub(crate) fn get(&self, number: &u32) -> Option<&T> {
        self.slots[self.position(*number)?]
            .as_ref()
            .map(|entry| &entry.value)
    }

    /// 保存した参照結果を再確保なしで更新する。
    pub(crate) fn get_mut(&mut self, number: &u32) -> Option<&mut T> {
        let slot = self.position(*number)?;
        self.slots[slot].as_mut().map(|entry| &mut entry.value)
    }

    /// 初回作成前に必要な slot を予約し、再確保の全容量を累積へ加える。
    pub(crate) fn prepare(&mut self, budget: &DecodeBudget, number: u32) -> Option<()> {
        if budget.is_stopped() {
            return None;
        }
        if self.get(&number).is_some() {
            return Some(());
        }
        if self.len >= self.slots.len() / 2 {
            let capacity = self.slots.len().saturating_mul(2).max(4);
            budget.charge_resource_workspace_bytes(
                (capacity as u64).saturating_mul(std::mem::size_of::<Option<Entry<T>>>() as u64),
            )?;
            let mut slots = budget.try_vector_capacity(capacity)?;
            slots.resize_with(capacity, || None);
            let old = std::mem::replace(&mut self.slots, slots);
            for entry in old.into_iter().flatten() {
                self.place(entry);
            }
        }
        Some(())
    }

    /// 最初の値を保存し、重複 key の値と容量を維持する。
    pub(crate) fn insert(&mut self, budget: &DecodeBudget, number: u32, value: T) -> Option<()> {
        self.prepare(budget, number)?;
        if self.get(&number).is_none() {
            self.place(Entry { number, value });
            self.len += 1;
        }
        Some(())
    }

    /// 予約済み配列の空 slot に値を移す。
    fn place(&mut self, entry: Entry<T>) {
        let mut slot = self.start(entry.number);
        while self.slots[slot].is_some() {
            slot = (slot + 1) & (self.slots.len() - 1);
        }
        self.slots[slot] = Some(entry);
    }

    /// テストで保存済み件数を検証する。
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// テストで超過時の未保存状態を検証する。
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits};
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    /// test value を含む一 slot の要求容量。
    const SLOT: u64 = std::mem::size_of::<Option<Entry<u32>>>() as u64;

    /// cache 専用の累積予算を作る。
    fn budget(limit: u64) -> DecodeBudget {
        DecodeBudget::new(&ParseLimits {
            max_total_resource_workspace_bytes: limit,
            ..Default::default()
        })
    }

    /// 再確保の累積境界と重複 key、停止後の不変性を検証する。
    #[test]
    fn growth_is_cumulative_inclusive_and_preserves_entries_on_failure() {
        for limit in [12 * SLOT - 1, 12 * SLOT] {
            let budget = budget(limit);
            let mut cache = Cache::default();
            assert_eq!(cache.insert(&budget, 0, 10), Some(()));
            assert_eq!(cache.insert(&budget, u32::MAX, 20), Some(()));
            assert_eq!(cache.insert(&budget, 0, 99), Some(()));
            assert_eq!(cache.get(&0), Some(&10));
            assert_eq!(cache.insert(&budget, 1, 30).is_some(), limit == 12 * SLOT);
            assert_eq!(cache.get(&u32::MAX), Some(&20));
            if limit < 12 * SLOT {
                assert!(
                    matches!(budget.check(), Err(ParseError::ResourceLimit { resource: "resource workspace bytes", actual, .. }) if actual == 12 * SLOT)
                );
                assert_eq!(cache.insert(&budget, 0, 99), None);
                assert_eq!(cache.len(), 2);
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 任意の番号順序/重複と複数回の拡張でも独立 map の最初の値と一致する。
        #[test]
        fn arbitrary_objects_match_first_value_model(keys in prop::collection::vec(any::<u32>(), 0..100)) {
            let budget = budget(u64::MAX);
            let mut cache = Cache::default();
            let mut model = BTreeMap::new();
            for (index, &key) in keys.iter().enumerate() {
                cache.insert(&budget, key, index as u32).unwrap();
                model.entry(key).or_insert(index as u32);
                for (&key, value) in &model { prop_assert_eq!(cache.get(&key), Some(value)); }
            }
            prop_assert_eq!(cache.len(), model.len());
            prop_assert!(budget.check().is_ok());
        }
    }
}
