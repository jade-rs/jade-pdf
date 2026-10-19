//! ページとFormの局所Font対応を容量制限付きmapへ保存する。
use super::{
    DecodeBudget, PageFontResourceDefinition, StreamOffsets, ToUnicodeCMap,
    font_cmap_for_object_number, font_names,
};

/// 1ページで有効なfont resource情報。
#[derive(Debug, Default)]
pub(super) struct Resources {
    /// resource名からPDF BaseFont名への対応。
    pub(super) base_fonts: crate::metadata_map::Map<String, String>,
    /// resource名からexternal CMap encoding名への対応。
    pub(super) encodings: crate::metadata_map::Map<String, String>,
    /// resource名からFont object固有ToUnicode CMapへの対応。
    pub(super) cmaps: crate::metadata_map::Map<String, ToUnicodeCMap>,
    /// resource 名から object ごとに共有する encoded glyph 幅への対応。
    pub(super) metrics:
        crate::metadata_map::Map<String, std::sync::Arc<crate::font_widths::Metrics>>,
}

/// 検査済み Font object の共有 map から、局所 resource 名への対応を作る。
#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    definitions: Vec<PageFontResourceDefinition<'_>>,
    base_fonts: &font_names::Names,
    encodings: &font_names::Names,
    cmaps: &super::font_cmaps::Index,
    stream_offsets: &StreamOffsets,
    metrics_cache: &mut crate::font_metrics_cache::Cache<'_, '_>,
) -> Resources {
    let objects = metrics_cache.objects();
    let mut page_resources = Resources::default();
    for definition in definitions {
        if decode_budget.is_stopped() {
            return Default::default();
        }
        let name = &definition.resource_name;
        let dictionary = definition
            .object_number
            .and_then(|n| objects.get(&n).map(|(_, body)| *body))
            .or_else(|| definition.inline_dictionary.map(str::as_bytes));
        if let Some(dictionary) = dictionary {
            if page_resources
                .metrics
                .prepare(decode_budget, name.as_str())
                .is_none()
            {
                break;
            }
            let metrics = match definition.object_number {
                Some(number) => metrics_cache.get(number),
                None => metrics_cache.read(dictionary),
            };
            if let Some(metrics) = metrics {
                crate::metadata_map::save(
                    decode_budget,
                    &mut page_resources.metrics,
                    name,
                    metrics,
                );
            }
        }
        if let Some(font_object) = definition.object_number {
            if let Some(base_font) = base_fonts.get(&font_object)
                && let Some(value) = crate::metadata_map::copy(decode_budget, base_font)
            {
                crate::metadata_map::save(
                    decode_budget,
                    &mut page_resources.base_fonts,
                    name,
                    value,
                );
            }
            if let Some(encoding) = encodings.get(&font_object)
                && let Some(value) = crate::metadata_map::copy(decode_budget, encoding)
            {
                crate::metadata_map::save(
                    decode_budget,
                    &mut page_resources.encodings,
                    name,
                    value,
                );
            }
            if let Some(cmap) = cmaps.get(&font_object) {
                crate::metadata_map::save(
                    decode_budget,
                    &mut page_resources.cmaps,
                    name,
                    cmap.clone(),
                );
            }
            continue;
        }
        let Some(dictionary) = definition.inline_dictionary else {
            continue;
        };
        if let Some(base_font) =
            crate::metadata_names::name(decode_budget, dictionary.as_bytes(), b"BaseFont")
        {
            crate::metadata_map::save(
                decode_budget,
                &mut page_resources.base_fonts,
                name,
                base_font,
            );
        }
        if let Some(encoding) =
            crate::metadata_names::name(decode_budget, dictionary.as_bytes(), b"Encoding")
        {
            crate::metadata_map::save(decode_budget, &mut page_resources.encodings, name, encoding);
        }
        if let Some(tounicode_object) = super::font_cmaps::reference(objects, dictionary.as_bytes())
            && let Some(cmap) =
                font_cmap_for_object_number(decode_budget, bytes, tounicode_object, stream_offsets)
            && !cmap.is_empty()
        {
            crate::metadata_map::save(decode_budget, &mut page_resources.cmaps, name, cmap);
        }
    }
    if decode_budget.is_stopped() {
        Default::default()
    } else {
        page_resources
    }
}
