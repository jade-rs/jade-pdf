//! 検証用配列・訪問集合・幅優先列を metadata の累積要求容量で制限する。
use crate::{ParseError, decode::DecodeBudget};
use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hash},
};

/// 予算超過・確保失敗を返し、消費済みの停止状態でも処理を再開させない。
fn required<T>(budget: &DecodeBudget, value: Option<T>) -> Result<T, ParseError> {
    if let Some(value) = value {
        return Ok(value);
    }
    budget.check()?;
    Err(ParseError::Io(std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "metadata workspace stopped",
    )))
}

/// 配列を必要としない検査も、消費済み予算エラー後には再開しない。
pub(crate) fn ensure_active(budget: &DecodeBudget) -> Result<(), ParseError> {
    required(budget, (!budget.is_stopped()).then_some(()))
}

/// 件数が既知の配列を確保前に予約し、停止・容量超過・確保失敗を返す。
pub(crate) fn vector<T>(budget: &DecodeBudget, capacity: usize) -> Result<Vec<T>, ParseError> {
    required(budget, budget.metadata_vector(capacity))
}

/// 初回と拡張時の全要求容量を予約し、既存要素を複製せず移動して追加する。
pub(crate) fn push<T>(
    budget: &DecodeBudget,
    values: &mut Vec<T>,
    value: T,
) -> Result<(), ParseError> {
    required(budget, try_push(budget, values, value))
}

/// Optionを返す抽出処理へ追加し、予算エラーはcheckまで保持する。
pub(crate) fn try_push<T>(budget: &DecodeBudget, values: &mut Vec<T>, value: T) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    if values.len() == values.capacity() {
        let capacity = values.capacity().saturating_mul(2).max(4);
        let mut replacement = budget.metadata_vector(capacity)?;
        replacement.append(values);
        *values = replacement;
    }
    values.push(value);
    Some(())
}

/// 属性・Kids の借用値配列を、共有予算に従って保存する。
pub(crate) fn array_values<'a>(
    budget: &DecodeBudget,
    bytes: &'a [u8],
    maximum: usize,
) -> Result<Option<Vec<&'a [u8]>>, ParseError> {
    if budget.is_stopped() {
        return required(budget, None);
    }
    let Some(entries) = crate::metadata::array_entries(bytes, maximum) else {
        return Ok(None);
    };
    let mut values = Vec::new();
    for value in entries {
        let Some(value) = value else {
            return Ok(None);
        };
        push(budget, &mut values, value)?;
    }
    Ok(Some(values))
}

/// generation を含む参照や Form 番号を、占有率半分以下の配列へ保存する集合。
pub(crate) struct Set<T> {
    /// 全配列の要求容量を確保前に累積予約する slot。
    slots: Vec<Option<T>>,
    /// 重複を除いた保存件数。
    len: usize,
    /// 未信頼キーの意図的な衝突を避ける seed。
    hash: RandomState,
}

impl<T> Default for Set<T> {
    /// 最初の参照まで配列を確保しない空集合を作る。
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            len: 0,
            hash: RandomState::new(),
        }
    }
}

impl<T: Hash + Eq + Copy> Set<T> {
    /// power of two の配列内でキーの探索開始位置を決める。
    fn start(&self, value: &T) -> usize {
        self.hash.hash_one(value) as usize & (self.slots.len() - 1)
    }

    /// 半分以下の占有率で有限な探索を行う。
    pub(crate) fn contains(&self, value: &T) -> bool {
        if self.slots.is_empty() {
            return false;
        }
        let mut slot = self.start(value);
        for _ in 0..self.slots.len() {
            match self.slots[slot] {
                Some(found) if found == *value => return true,
                None => return false,
                _ => slot = (slot + 1) & (self.slots.len() - 1),
            }
        }
        false
    }

    /// 予約済み配列の空 slot へ一つの値を保存する。
    fn place(&mut self, value: T) {
        let mut slot = self.start(&value);
        while self.slots[slot].is_some() {
            slot = (slot + 1) & (self.slots.len() - 1);
        }
        self.slots[slot] = Some(value);
    }

    /// 重複は無確保で返し、拡張前に全配列容量を予約する。
    pub(crate) fn insert(&mut self, budget: &DecodeBudget, value: T) -> Result<bool, ParseError> {
        required(budget, self.try_insert(budget, value))
    }

    /// Option抽出では予算エラーを消費せず、重複は無確保で返す。
    pub(crate) fn try_insert(&mut self, budget: &DecodeBudget, value: T) -> Option<bool> {
        if budget.is_stopped() {
            return None;
        }
        if self.contains(&value) {
            return Some(false);
        }
        if self.len >= self.slots.len() / 2 {
            let capacity = self.slots.len().saturating_mul(2).max(4);
            let mut slots = budget.metadata_vector(capacity)?;
            slots.resize_with(capacity, || None);
            let old = std::mem::replace(&mut self.slots, slots);
            for value in old.into_iter().flatten() {
                self.place(value);
            }
        }
        self.place(value);
        self.len += 1;
        Some(true)
    }

    /// 保存済みの値がないことを追加走査なしで返す。
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 節点数・参照深度の上限検査用の保存件数を返す。
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

/// 共有 Form の最短深度を保つ、容量制限付きの FIFO ring。
pub(crate) struct Queue<T> {
    /// 処理済み slot を再利用し、拡張時だけ確保する配列。
    slots: Vec<Option<T>>,
    /// 次に取り出す slot の位置。
    head: usize,
    /// 未処理要素数。
    len: usize,
}

impl<T> Default for Queue<T> {
    /// 初回追加まで配列を確保しない作業列を作る。
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            head: 0,
            len: 0,
        }
    }
}

impl<T> Queue<T> {
    /// 処理済み slot を再利用し、拡張の全要求容量を確保前に予約する。
    pub(crate) fn push(&mut self, budget: &DecodeBudget, value: T) -> Result<(), ParseError> {
        if budget.is_stopped() {
            return required(budget, None);
        }
        if self.len == self.slots.len() {
            let capacity = self.slots.len().saturating_mul(2).max(4);
            let mut slots = required(budget, budget.metadata_vector(capacity))?;
            slots.resize_with(capacity, || None);
            let old_capacity = self.slots.len();
            for (index, target) in slots.iter_mut().take(self.len).enumerate() {
                *target = self.slots[(self.head + index) % old_capacity].take();
            }
            self.slots = slots;
            self.head = 0;
        }
        let tail = (self.head + self.len) % self.slots.len();
        self.slots[tail] = Some(value);
        self.len += 1;
        Ok(())
    }

    /// 先頭要素を移動し、slot を次の追加へ再利用する。
    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let value = self.slots[self.head].take();
        self.head = (self.head + 1) % self.slots.len();
        self.len -= 1;
        value
    }
}

#[cfg(test)]
#[path = "metadata_workspace_tests.rs"]
mod tests;
