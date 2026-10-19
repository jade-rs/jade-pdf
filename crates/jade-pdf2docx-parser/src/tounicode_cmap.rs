//! ToUnicode の保存配列を予算内で構築し、完成 lookup を共有する。
use crate::decode::DecodeBudget;
use std::{
    ops::{Deref, Index},
    sync::Arc,
};

/// 完成した ToUnicode CMap の不変 lookup 情報を共有する。
#[derive(Debug, Clone, Default)]
pub(crate) struct ToUnicodeCMap {
    /// 非空mapの共有保存領域。空mapはheapを使わず静的領域を参照する。
    data: Option<Arc<ToUnicodeCMapData>>,
}

/// ToUnicode CMap の構築中だけ変更できる lookup 情報。
#[derive(Debug, Default)]
pub(crate) struct ToUnicodeCMapData {
    /// 完成時に key 順へ整列する文字対応。
    pub(super) mappings: CMapMappings,
    /// 完成時に整列・重複除去する許可 CID 範囲。
    pub(super) code_space_ranges: Vec<CodeSpaceRange>,
}

/// lookup record を連続領域へ保存する配列。
#[derive(Debug, Default)]
pub(super) struct CMapMappings {
    /// 構築中は入力順、完成後は key 順の文字対応。
    entries: Vec<MappingEntry>,
}

/// 文字対応と重複 key の優先順位。
#[derive(Debug)]
struct MappingEntry {
    /// 正規化済み CID hex。
    key: String,
    /// 復号済み Unicode。
    value: String,
    /// 後の assignment を優先する入力順序。
    order: usize,
}

/// `begincodespacerange` の許可 CID 範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodeSpaceRange {
    /// CID hex の桁幅。
    pub(super) width: usize,
    /// 許可する開始値。
    pub(super) start: u32,
    /// 許可する終了値。
    pub(super) end: u32,
}

impl CMapMappings {
    /// 整列済み key を二分探索する。
    pub(super) fn get(&self, key: &str) -> Option<&String> {
        let index = self
            .entries
            .binary_search_by(|entry| entry.key.as_str().cmp(key))
            .ok()?;
        Some(&self.entries[index].value)
    }

    /// key 順に保存された文字対応を読む。
    pub(super) fn iter(&self) -> impl Iterator<Item = (&String, &String)> + Clone {
        self.entries.iter().map(|entry| (&entry.key, &entry.value))
    }

    /// key を昇順で読む。
    pub(super) fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|entry| &entry.key)
    }

    /// 保存された文字対応の件数を返す。
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// 保存された文字対応が空か返す。
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Index<&str> for CMapMappings {
    type Output = String;
    /// 存在する key の値を参照する。
    fn index(&self, key: &str) -> &Self::Output {
        self.get(key).expect("ToUnicode key must exist")
    }
}

impl Deref for ToUnicodeCMap {
    type Target = ToUnicodeCMapData;
    /// 完成 map を読み取り専用で参照する。
    fn deref(&self) -> &Self::Target {
        /// 初回も反復時もheapを確保しない空mapの保存領域。
        static EMPTY: ToUnicodeCMapData = ToUnicodeCMapData {
            mappings: CMapMappings {
                entries: Vec::new(),
            },
            code_space_ranges: Vec::new(),
        };
        self.data.as_deref().unwrap_or(&EMPTY)
    }
}

impl ToUnicodeCMap {
    /// 完成 map の保存領域を共有しているか検査する。
    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        std::ptr::eq(self.deref(), other.deref())
    }

    /// CMap に文字対応が無いか返す。
    pub(crate) fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// 単一 BMP 文字の対応だけを font cmap 修復用に返す。
    pub(crate) fn bmp_scalar_iter(&self) -> impl Iterator<Item = (u16, u16)> + Clone + '_ {
        self.mappings.iter().filter_map(|(source, value)| {
            let source = u16::from_str_radix(source, 16).ok()?;
            let mut chars = value.chars();
            let unicode = chars.next()? as u32;
            if chars.next().is_some() || unicode >= 0xffff || (0xd800..=0xdfff).contains(&unicode) {
                return None;
            }
            Some((source, unicode as u16))
        })
    }

    /// テスト比較用の単一 BMP 対応を返す。
    #[cfg(test)]
    pub(crate) fn bmp_scalar_mappings(&self) -> Vec<(u16, u16)> {
        self.bmp_scalar_iter().collect()
    }

    /// CID hex が code space で許可されているか返す。
    pub(super) fn key_is_allowed(&self, key: &str) -> bool {
        if self.code_space_ranges.is_empty() {
            return true;
        }
        let Some(value) = u32::from_str_radix(key, 16).ok() else {
            return false;
        };
        self.code_space_ranges
            .iter()
            .any(|range| range.width == key.len() && value >= range.start && value <= range.end)
    }

    /// code space で許可された CID hex 桁幅を返す。
    #[cfg(test)]
    pub(super) fn code_space_widths(&self) -> Vec<usize> {
        let mut widths = self
            .code_space_ranges
            .iter()
            .map(|range| range.width)
            .collect::<Vec<_>>();
        widths.sort_unstable_by(|a, b| b.cmp(a));
        widths.dedup();
        widths
    }
}

impl ToUnicodeCMapData {
    /// 整列・重複除去後に共有本体を予約し、停止時は部分結果を破棄する。
    pub(super) fn finish(mut self, budget: &DecodeBudget) -> ToUnicodeCMap {
        if budget.is_stopped() {
            return ToUnicodeCMap::default();
        }
        self.mappings
            .entries
            .sort_unstable_by(|a, b| a.key.cmp(&b.key).then_with(|| b.order.cmp(&a.order)));
        self.mappings.entries.dedup_by(|a, b| a.key == b.key);
        self.code_space_ranges
            .sort_unstable_by_key(|range| (range.width, range.start, range.end));
        self.code_space_ranges.dedup();
        if self.mappings.is_empty() && self.code_space_ranges.is_empty() {
            ToUnicodeCMap::default()
        } else {
            ToUnicodeCMap {
                data: budget.budgeted_arc(self),
            }
        }
    }

    /// key/value を作る前に一件分の record 容量を確保する。
    pub(super) fn reserve_mapping(&mut self, budget: &DecodeBudget) -> Option<()> {
        reserve_storage(budget, &mut self.mappings.entries, 1)
    }

    /// 予約済みの配列へ文字対応を入力順に追加する。
    pub(super) fn insert_mapping(&mut self, key: String, value: String) {
        let order = self.mappings.entries.len();
        self.mappings
            .entries
            .push(MappingEntry { key, value, order });
    }

    /// 有効な許可 CID 範囲を予算内で追加する。
    pub(super) fn insert_code_space_range(
        &mut self,
        budget: &DecodeBudget,
        start_hex: &str,
        end_hex: &str,
    ) -> Option<()> {
        let width = start_hex.len() + start_hex.len() % 2;
        if width != end_hex.len() + end_hex.len() % 2 || width == 0 {
            return Some(());
        }
        let (Some(start), Some(end)) =
            (normalized_hex_code(start_hex), normalized_hex_code(end_hex))
        else {
            return Some(());
        };
        if start > end {
            return Some(());
        }
        reserve_storage(budget, &mut self.code_space_ranges, 1)?;
        self.code_space_ranges
            .push(CodeSpaceRange { width, start, end });
        Some(())
    }

    /// 別の構築中 CMap を移し、後の stream の assignment を優先する。
    pub(super) fn extend(&mut self, budget: &DecodeBudget, mut other: Self) -> Option<()> {
        reserve_storage(
            budget,
            &mut self.mappings.entries,
            other.mappings.entries.len(),
        )?;
        reserve_storage(
            budget,
            &mut self.code_space_ranges,
            other.code_space_ranges.len(),
        )?;
        let offset = self.mappings.entries.len();
        for (index, entry) in other.mappings.entries.iter_mut().enumerate() {
            entry.order = offset + index;
        }
        self.mappings.entries.append(&mut other.mappings.entries);
        self.code_space_ranges.append(&mut other.code_space_ranges);
        Some(())
    }

    /// 完成 stream の payload を予算内で複製し、後の stream を優先する。
    pub(super) fn extend_shared(
        &mut self,
        budget: &DecodeBudget,
        other: &ToUnicodeCMap,
    ) -> Option<()> {
        reserve_storage(
            budget,
            &mut self.mappings.entries,
            other.mappings.entries.len(),
        )?;
        reserve_storage(
            budget,
            &mut self.code_space_ranges,
            other.code_space_ranges.len(),
        )?;
        for (key, value) in other.mappings.iter() {
            budget.charge_tounicode_bytes((key.len() as u64).saturating_add(value.len() as u64))?;
            let mut copied_key = budget.try_string_capacity(key.len())?;
            copied_key.push_str(key);
            let mut copied_value = budget.try_string_capacity(value.len())?;
            copied_value.push_str(value);
            self.insert_mapping(copied_key, copied_value);
        }
        self.code_space_ranges
            .extend_from_slice(&other.code_space_ranges);
        Some(())
    }
}

/// 配列の新容量全体を累積予約し、旧領域と共存する再確保も数える。
fn reserve_storage<T>(budget: &DecodeBudget, values: &mut Vec<T>, additional: usize) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let needed = values.len().saturating_add(additional);
    if needed <= values.capacity() {
        return Some(());
    }
    let capacity = needed.max(values.capacity().saturating_mul(2)).max(1);
    budget.charge_tounicode_storage_bytes(
        (capacity as u64).saturating_mul(std::mem::size_of::<T>() as u64),
    )?;
    let mut fresh = budget.try_vector_capacity(capacity)?;
    fresh.append(values);
    *values = fresh;
    Some(())
}

/// 奇数桁 code space の末尾ゼロ補完を String 確保なしで読む。
fn normalized_hex_code(hex: &str) -> Option<u32> {
    let value = u32::from_str_radix(hex, 16).ok()?;
    if !hex.len().is_multiple_of(2) {
        value.checked_mul(16)
    } else {
        Some(value)
    }
}
