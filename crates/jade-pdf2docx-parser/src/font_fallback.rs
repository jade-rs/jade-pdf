//! Rootなし互換入力の全体Font対応を作る。
use crate::decode::DecodeBudget;
use crate::metadata_map::Map;

/// fallbackの値を複製し、文字列payloadだけをmetadata予算へ数える。
pub(crate) trait Value {
    /// 保存する値を作り、共有CMapのpayloadは複製しない。
    fn saved(&self, budget: &DecodeBudget) -> Option<Self>
    where
        Self: Sized;
}
impl Value for String {
    /// UTF-8全byte容量を確保前に予約する。
    fn saved(&self, budget: &DecodeBudget) -> Option<Self> {
        crate::metadata_map::copy(budget, self)
    }
}
impl Value for crate::text::ToUnicodeCMap {
    /// 完成済みCMapの共有所有者だけを増やす。
    fn saved(&self, budget: &DecodeBudget) -> Option<Self> {
        (!budget.is_stopped()).then(|| self.clone())
    }
}

/// object番号順に互換参照を検索し、容量停止時は部分mapを全て破棄する。
pub(crate) fn build<V: Value>(
    budget: &DecodeBudget,
    source: &str,
    values: impl IntoIterator<Item = (u32, u16, V)>,
) -> Map<String, V> {
    let mut result = Map::default();
    if budget.is_stopped() {
        return result;
    }
    for (number, generation, value) in values {
        for (raw, reference, reference_generation) in
            crate::font_references::named(source.as_bytes())
        {
            if reference == number
                && reference_generation == generation
                && let Some(name) = crate::metadata_names::resource_name(budget, raw)
                && result.prepare(budget, name.as_str()).is_some()
                && let Some(value) = value.saved(budget)
            {
                result.insert(budget, name, value);
            }
            if budget.is_stopped() {
                return Map::default();
            }
        }
    }
    result
}

#[cfg(test)]
#[path = "font_fallback_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "font_fallback_syntax_tests.rs"]
mod syntax_tests;
