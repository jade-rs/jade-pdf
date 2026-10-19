//! 不変Form visitor内だけで、同じResourcesの完成Font mapを共有する。
use super::{DecodeBudget, PageFontResources};

/// visitorのsourceが生きている間に限り有効なResourcesの位置と長さ。
type Key = (usize, usize);

/// 同じ局所辞書のFont mapを一度だけ作成するcache。
#[derive(Default)]
pub(super) struct Cache {
    /// Resourcesの位置をキーにする予約済みslot。入力所有者より先に破棄する。
    entries: crate::metadata_map::Map<Key, PageFontResources>,
}
impl Cache {
    /// 構築済みの辞書を共有借用し、複数の選択元を同時に参照できるようにする。
    pub(super) fn get(&self, resources: &[u8]) -> Option<&PageFontResources> {
        self.entries
            .get(&(resources.as_ptr() as usize, resources.len()))
    }
    /// missのslotを作成前に予約し、停止したfactoryの結果と停止後のhitを公開しない。
    pub(super) fn get_or_build(
        &mut self,
        budget: &DecodeBudget,
        resources: &[u8],
        build: impl FnOnce() -> PageFontResources,
    ) -> Option<&PageFontResources> {
        if budget.is_stopped() {
            return None;
        }
        let key = (resources.as_ptr() as usize, resources.len());
        if self.entries.get(&key).is_none() {
            self.entries.prepare(budget, &key)?;
            let fonts = build();
            if budget.is_stopped() {
                return None;
            }
            self.entries.insert(budget, key, fonts)?;
        }
        self.entries.get(&key)
    }
}

#[cfg(test)]
#[path = "font_local_cache_tests.rs"]
mod tests;
