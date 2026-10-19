//! metadataのmap容量をslot配列の確保前に累積予約する。
use crate::decode::DecodeBudget;
use std::{
    borrow::Borrow,
    collections::{BTreeMap, hash_map::RandomState},
    hash::{BuildHasher, Hash},
};

/// 未信頼キーの衝突をseedで避け、占有率を半分以下に保つmap。
#[derive(Debug)]
pub(crate) struct Map<K, V> {
    /// キーと値の保存slot。拡張時は旧内容を移動する。
    slots: Vec<Option<(K, V)>>,
    /// 重複を除いた件数。
    len: usize,
    /// 入力ごとに異なるhash seed。
    hash: RandomState,
}
impl<K, V> Default for Map<K, V> {
    /// 初回保存まで確保しない空mapを作る。
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            len: 0,
            hash: RandomState::new(),
        }
    }
}
impl<K: Hash + Eq, V> Map<K, V> {
    /// 借用キーを有限回のprobeで検索する。
    fn find<Q: Hash + Eq + ?Sized>(&self, key: &Q) -> Option<usize>
    where
        K: Borrow<Q>,
    {
        if self.slots.is_empty() {
            return None;
        }
        let mut slot = self.hash.hash_one(key) as usize & (self.slots.len() - 1);
        for _ in 0..self.slots.len() {
            match &self.slots[slot] {
                Some((found, _)) if found.borrow() == key => return Some(slot),
                None => return None,
                _ => slot = (slot + 1) & (self.slots.len() - 1),
            }
        }
        None
    }
    /// 保存済みの値を借用し、再確保しない。
    pub(crate) fn get<Q: Hash + Eq + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.find(key)
            .and_then(|slot| self.slots[slot].as_ref().map(|(_, value)| value))
    }
    /// テストで保存件数が空であることを検証する。
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// 完成配列の容量を事前予約するために、保存件数を返す。
    pub(crate) fn len(&self) -> usize {
        self.len
    }
    /// 予約済みslotからentryを移動し、追加の収集配列を作らない。
    pub(crate) fn into_entries(self) -> impl Iterator<Item = (K, V)> {
        self.slots.into_iter().flatten()
    }
    /// 予約済み配列へ重複しないentryを移動する。
    fn place(&mut self, key: K, value: V) {
        let mut slot = self.hash.hash_one(&key) as usize & (self.slots.len() - 1);
        while self.slots[slot].is_some() {
            slot = (slot + 1) & (self.slots.len() - 1);
        }
        self.slots[slot] = Some((key, value));
    }
    /// キーや値を作る前にslot容量を検査し、拡張の全容量を累積予約する。
    pub(crate) fn prepare<Q: Hash + Eq + ?Sized>(
        &mut self,
        budget: &DecodeBudget,
        key: &Q,
    ) -> Option<()>
    where
        K: Borrow<Q>,
    {
        if budget.is_stopped() {
            return None;
        }
        if self.find(key).is_some() {
            return Some(());
        }
        if self.len >= self.slots.len() / 2 {
            let capacity = self.slots.len().saturating_mul(2).max(4);
            let mut slots = budget.metadata_vector(capacity)?;
            slots.resize_with(capacity, || None);
            let old = std::mem::replace(&mut self.slots, slots);
            for (key, value) in old.into_iter().flatten() {
                self.place(key, value);
            }
        }
        Some(())
    }
    /// 重複キーは後出しの値を選び、予算停止時には保存しない。
    pub(crate) fn insert(&mut self, budget: &DecodeBudget, key: K, value: V) -> Option<()> {
        self.prepare(budget, &key)?;
        if let Some(slot) = self.find(&key) {
            self.slots[slot] = Some((key, value));
        } else {
            self.place(key, value);
            self.len += 1;
        }
        Some(())
    }
}

/// 局所mapと旧fallback索引に共通の文字列キー検索。
pub(crate) trait Lookup<V> {
    /// 名前に対応する値を借用する。
    fn get(&self, key: &str) -> Option<&V>;
}
impl<V> Lookup<V> for BTreeMap<String, V> {
    /// 既存のfallbackやfixtureの値を借用する。
    fn get(&self, key: &str) -> Option<&V> {
        BTreeMap::get(self, key)
    }
}
impl<V> Lookup<V> for Map<String, V> {
    /// 予約済みslotから値を借用する。
    fn get(&self, key: &str) -> Option<&V> {
        Map::get(self, key)
    }
}

/// UTF-8文字列の全byte容量を予約してから複製する。
pub(crate) fn copy(budget: &DecodeBudget, text: &str) -> Option<String> {
    let mut bytes = budget.metadata_vector(text.len())?;
    bytes.extend_from_slice(text.as_bytes());
    String::from_utf8(bytes).ok()
}

/// 局所名のslotと複製容量を予約し、値を保存する。
pub(crate) fn save<V>(
    budget: &DecodeBudget,
    map: &mut Map<String, V>,
    name: &str,
    value: V,
) -> Option<()> {
    map.prepare(budget, name)?;
    let key = copy(budget, name)?;
    map.insert(budget, key, value)
}

#[cfg(test)]
impl<V> std::ops::Index<&str> for Map<String, V> {
    type Output = V;
    /// 既存fixtureの検査を、確保しない名前検索へ接続する。
    fn index(&self, key: &str) -> &V {
        self.get(key).expect("missing map key")
    }
}
#[cfg(test)]
impl<K: Hash + Eq, V> Map<K, V> {
    /// fixtureが名前の有無だけを確認するための借用検索。
    pub(crate) fn contains_key<Q: Hash + Eq + ?Sized>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.find(key).is_some()
    }
}

#[cfg(test)]
#[path = "metadata_map_tests.rs"]
mod tests;
