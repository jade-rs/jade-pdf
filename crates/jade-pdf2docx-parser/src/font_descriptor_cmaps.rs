//! FontDescriptorとFontの完成CMapを共有する対応を作る。
use super::{DecodeBudget, ToUnicodeCMap};
use crate::metadata_map::Map;

/// 入力順の最初のBMP対応を予約済みslotへ共有し、停止時は部分map全体を捨てる。
pub(super) fn build<'a>(
    budget: &DecodeBudget,
    source: &[u8],
    mut cmap: impl FnMut(u32) -> Option<&'a ToUnicodeCMap>,
) -> Map<u32, ToUnicodeCMap> {
    let mut mappings = Map::default();
    if budget.is_stopped() {
        return mappings;
    }
    for object in crate::metadata::objects(source, None) {
        if crate::metadata::has_type(object.body, b"Font")
            && let Some(descriptor) = crate::metadata::value(object.body, b"FontDescriptor")
                .and_then(crate::metadata::reference)
            && let Some(cmap) = cmap(object.number)
            && cmap.bmp_scalar_iter().next().is_some()
            && mappings.get(&descriptor).is_none()
            && mappings.prepare(budget, &descriptor).is_some()
        {
            mappings.insert(budget, descriptor, cmap.clone());
        }
        if budget.is_stopped() {
            return Map::default();
        }
    }
    mappings
}

#[cfg(test)]
#[path = "font_descriptor_cmaps_tests.rs"]
mod tests;
