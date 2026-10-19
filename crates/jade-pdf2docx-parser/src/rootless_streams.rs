//! Rootなし互換PDFのFlate本文を、予約済みmapと番号順の最終配列へ保存する。
use super::{
    DecodeBudget, StreamOffsets, decode_zlib, extract_stream_object_at_offset, find_bytes,
    parse_object_number_before_stream,
};
use crate::metadata_map::Map;

/// legacyの番号優先順位を保持し、容量不足なら先行本文も破棄する。
pub(super) fn extract(budget: &DecodeBudget, bytes: &[u8]) -> Vec<(usize, Vec<u8>)> {
    if budget.is_stopped()
        || budget
            .stream_index(bytes)
            .is_some_and(|index| index.has_xref())
    {
        return Vec::new();
    }
    collect(budget, bytes).unwrap_or_default()
}

/// 全保存slotと最終配列を予約し、payload自体は展開予算で制限する。
fn collect(budget: &DecodeBudget, bytes: &[u8]) -> Option<Vec<(usize, Vec<u8>)>> {
    let mut streams = Map::default();
    for (number, offset) in StreamOffsets::legacy(budget, bytes).into_pairs() {
        let Some((dict, data)) = extract_stream_object_at_offset(budget, bytes, offset) else {
            continue;
        };
        if is_flate(dict) {
            streams.prepare(budget, &number)?;
            if let Some(decoded) = decode_zlib(data, budget) {
                streams.insert(budget, number, decoded)?;
            }
            if budget.is_stopped() {
                return None;
            }
        }
    }
    if budget.is_stopped() {
        return None;
    }
    if let Some(index) = budget.stream_index(bytes) {
        for stream in index.streams(bytes) {
            if is_flate(stream.dictionary)
                && let Some(number) = stream.object_number
            {
                save(budget, &mut streams, number, stream.payload)?;
            }
        }
    } else {
        let mut cursor = 0usize;
        while let Some(relative) = find_bytes(&bytes[cursor..], b"stream") {
            let position = cursor + relative;
            let dict_start = bytes[..position]
                .windows(2)
                .rposition(|window| window == b"<<");
            let dict = dict_start
                .map(|start| &bytes[start..position])
                .unwrap_or_default();
            let Some((data, next)) = crate::legacy_stream::payload(bytes, dict, position) else {
                break;
            };
            if is_flate(dict)
                && let Some(number) = parse_object_number_before_stream(bytes, position)
            {
                save(budget, &mut streams, number, data)?;
            }
            cursor = next;
        }
    }
    if budget.is_stopped() {
        return None;
    }
    let mut result = budget.metadata_vector(streams.len())?;
    for (number, data) in streams.into_entries() {
        result.push((number as usize, data));
    }
    result.sort_unstable_by_key(|entry| entry.0);
    for (page, entry) in result.iter_mut().enumerate() {
        entry.0 = page;
    }
    Some(result)
}

/// 既存と同じFlate判定を行い、非圧縮streamを対象外とする。
fn is_flate(dict: &[u8]) -> bool {
    dict.windows(b"FlateDecode".len())
        .any(|window| window == b"FlateDecode")
}

/// legacyで選んだ本文を優先し、重複本文の再展開・保存を避ける。
fn save(
    budget: &DecodeBudget,
    streams: &mut Map<u32, Vec<u8>>,
    number: u32,
    data: &[u8],
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    if streams.get(&number).is_some() {
        return Some(());
    }
    streams.prepare(budget, &number)?;
    if let Some(decoded) = decode_zlib(data, budget) {
        streams.insert(budget, number, decoded)?;
    }
    (!budget.is_stopped()).then_some(())
}
