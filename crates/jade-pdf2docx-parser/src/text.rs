use std::borrow::Cow;
#[cfg(test)]
use std::collections::BTreeMap;

use crate::decode::{DecodeBudget, decode_zlib};
#[cfg(test)]
use encoding_rs::{BIG5, EUC_KR, GBK, SHIFT_JIS};

#[path = "tounicode.rs"]
mod tounicode;
use tounicode::parse_tounicode_cmap_from_bytes;

#[cfg(test)]
#[path = "text_operand_budget_tests.rs"]
mod operand_budget_tests;

#[cfg(test)]
#[path = "text_code_budget_tests.rs"]
mod code_budget_tests;

#[cfg(test)]
#[path = "preview_stream_budget_tests.rs"]
mod preview_stream_budget_tests;

#[cfg(test)]
#[path = "preview_result_budget_tests.rs"]
mod preview_result_budget_tests;

#[path = "preview_candidates.rs"]
mod preview_candidates;

#[path = "preview_rendering.rs"]
mod preview_rendering;

#[path = "preview_result.rs"]
mod preview_result;

#[cfg(test)]
#[path = "preview_candidate_budget_tests.rs"]
mod preview_candidate_budget_tests;

#[path = "text_operand.rs"]
mod operand;

#[path = "tounicode_input.rs"]
mod cmap_input;

#[cfg(test)]
#[path = "tounicode_limits_tests.rs"]
mod cmap_limits_tests;

#[cfg(test)]
#[path = "tounicode_shared_tests.rs"]
mod cmap_shared_tests;

#[cfg(test)]
#[path = "tounicode_storage_tests.rs"]
mod cmap_storage_tests;

#[cfg(test)]
#[path = "tounicode_input_tests.rs"]
mod cmap_input_tests;

#[cfg(test)]
#[path = "tounicode_owned_allocation_tests.rs"]
mod cmap_owned_allocation_tests;

#[cfg(test)]
#[path = "tounicode_reference_budget_tests.rs"]
mod cmap_reference_budget_tests;

#[cfg(test)]
#[path = "tounicode_stream_lookup_tests.rs"]
mod cmap_stream_lookup_tests;

#[path = "tounicode_stream_lookup.rs"]
mod cmap_stream_lookup;

#[path = "tounicode_references.rs"]
mod cmap_references;
use cmap_references::parse as parse_tounicode_object_refs;

#[path = "tounicode_cmap.rs"]
mod cmap;
pub(crate) use cmap::{ToUnicodeCMap, ToUnicodeCMapData};

/// ページ・Formの可視本文を借用し、文書全体の重複除去・文字数制限を適用する。
pub(crate) fn preview_from_text_elements(
    budget: &DecodeBudget,
    elements: &[crate::ParsedTextElement],
) -> String {
    let mut builder = preview_result::Builder::default();
    for element in elements {
        match builder.push_decoded(budget, &element.text) {
            Some(true) => {}
            Some(false) => break,
            None => return String::new(),
        }
    }
    builder.finish(budget).unwrap_or_default()
}

/// PDF バイト列から最小限のテキストプレビューを抽出する。
///
/// 借用字句で表示operandを逐次復号し、容量を予約して可読候補を連結する。
/// 容量不足では部分結果全体を破棄する。
pub(crate) fn extract_text_preview_from_pdf_bytes(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> String {
    if decode_budget.is_stopped() {
        return String::new();
    }
    let cmap = parse_tounicode_cmap(decode_budget, bytes);
    let font_encodings = parse_font_resource_encodings(decode_budget, bytes);
    let metadata = if decode_budget.stream_index(bytes).is_some() {
        decode_budget.structural_bytes(bytes).into_owned()
    } else {
        pdf_bytes_without_stream_payloads(decode_budget, bytes)
    };
    let sources =
        std::iter::once(metadata).chain(extract_non_image_stream_payloads(decode_budget, bytes));
    if decode_budget.is_stopped() {
        return String::new();
    }
    let mut builder = preview_result::Builder::default();
    for stream in sources {
        for mode in [
            preview_candidates::Mode::TextObjects,
            preview_candidates::Mode::Font,
            preview_candidates::Mode::Shown,
        ] {
            match preview_candidates::visit(
                decode_budget,
                &stream,
                &cmap,
                &font_encodings,
                mode,
                |text| builder.push(decode_budget, text),
            ) {
                Some(true) => {}
                Some(false) => return builder.finish(decode_budget).unwrap_or_default(),
                None => return String::new(),
            }
        }
    }
    builder.finish(decode_budget).unwrap_or_default()
}

/// PDF 全体から font resource 名と external CMap encoding 名の対応を読む。
pub(crate) fn parse_font_resource_encodings(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> crate::metadata_map::Map<String, String> {
    let structural_bytes = decode_budget.structural_bytes(bytes);
    // 未検査streamを字句解析せず、escape名の可能性がある入力は従来探索を行う。
    if find_bytes(&structural_bytes, b"/Encoding").is_none() && !structural_bytes.contains(&b'#') {
        return Default::default();
    }
    let Some(source) = crate::metadata_input::text(decode_budget, &structural_bytes) else {
        return Default::default();
    };
    let encoding_by_object = crate::parser::font_object_encodings(decode_budget, &source);

    crate::font_fallback::build(decode_budget, &source, encoding_by_object)
}

/// PDF 内の ToUnicode CMap を集約して hex CID から Unicode 文字列への対応を作る。
pub(crate) fn parse_tounicode_cmap(decode_budget: &DecodeBudget, bytes: &[u8]) -> ToUnicodeCMap {
    if decode_budget.is_stopped() {
        return ToUnicodeCMap::default();
    }
    if let Some(cmap) = decode_budget.cached_global_cmap(bytes) {
        return cmap;
    }
    if decode_budget
        .charge_tounicode_input_bytes(bytes.len() as u64)
        .is_none()
    {
        return ToUnicodeCMap::default();
    }
    let structural_bytes = decode_budget.structural_bytes(bytes);
    let source = structural_bytes.as_ref();
    if find_bytes(source, b"/ToUnicode").is_none()
        && find_bytes(source, b"beginbfchar").is_none()
        && find_bytes(source, b"beginbfrange").is_none()
    {
        let empty = ToUnicodeCMap::default();
        decode_budget.cache_global_cmap(bytes, empty.clone());
        return empty;
    }
    let mut cmap = if find_bytes(source, b"beginbfchar").is_some()
        || find_bytes(source, b"beginbfrange").is_some()
    {
        parse_tounicode_cmap_from_bytes(decode_budget, source)
    } else {
        ToUnicodeCMapData::default()
    };
    if decode_budget.is_stopped() {
        return cmap.finish(decode_budget);
    }
    if decode_budget.stream_index(bytes).is_some() {
        return aggregate_checked_cmaps(decode_budget, bytes, source, cmap);
    }
    for object_number in parse_tounicode_object_refs(decode_budget, source) {
        if decode_budget.is_stopped() {
            break;
        }
        if let Some(stream) = extract_stream_payload_for_object(decode_budget, bytes, object_number)
        {
            let other = parse_tounicode_cmap_from_bytes(decode_budget, &stream);
            if cmap.extend(decode_budget, other).is_none() {
                break;
            }
        }
    }
    cmap.finish(decode_budget)
}

/// 完成 stream を入力順に集約し、単一 stream は保存領域をそのまま共有する。
fn aggregate_checked_cmaps(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    source: &[u8],
    mut cmap: ToUnicodeCMapData,
) -> ToUnicodeCMap {
    let mut pending = None;
    let mut merging = find_bytes(source, b"beginbfchar").is_some()
        || find_bytes(source, b"beginbfrange").is_some();
    for number in parse_tounicode_object_refs(decode_budget, source) {
        let Some(other) = parse_tounicode_stream(decode_budget, bytes, number) else {
            if decode_budget.is_stopped() {
                break;
            }
            continue;
        };
        if !merging {
            if let Some(first) = pending.take() {
                if cmap.extend_shared(decode_budget, &first).is_none() {
                    break;
                }
                merging = true;
            } else {
                pending = Some(other);
                continue;
            }
        }
        if cmap.extend_shared(decode_budget, &other).is_none() {
            break;
        }
    }
    if decode_budget.is_stopped() {
        return ToUnicodeCMap::default();
    }
    let result = pending.unwrap_or_else(|| cmap.finish(decode_budget));
    decode_budget.cache_global_cmap(bytes, result.clone());
    result
}

/// 検査済み文書の stream を一度だけ展開・解析し、完成 map を共有する。
pub(crate) fn parse_tounicode_stream(
    budget: &DecodeBudget,
    bytes: &[u8],
    number: u32,
) -> Option<ToUnicodeCMap> {
    if budget.is_stopped() {
        return None;
    }
    if let Some(cmap) = budget.cached_cmap(bytes, number) {
        return Some(cmap);
    }
    let payload = extract_stream_payload_for_object(budget, bytes, number)?;
    let cmap = parse_tounicode_body(budget, &payload);
    budget.cache_cmap(bytes, number, cmap.clone())?;
    Some(cmap)
}

/// 参照先が確定した CMap 本文を選択走査なしで解析する。
pub(crate) fn parse_tounicode_body(budget: &DecodeBudget, bytes: &[u8]) -> ToUnicodeCMap {
    parse_tounicode_cmap_from_bytes(budget, bytes).finish(budget)
}

/// 指定 object number の stream payload を必要なら FlateDecode して返す。
fn extract_stream_payload_for_object<'a>(
    decode_budget: &DecodeBudget,
    bytes: &'a [u8],
    object_number: u32,
) -> Option<Cow<'a, [u8]>> {
    if decode_budget.is_stopped() {
        return None;
    }
    if let Some(index) = decode_budget.stream_index(bytes) {
        let (dict, data) = index.for_object(bytes, object_number)?;
        return decode_cmap_stream(decode_budget, dict, data);
    }
    let (dict, data) = cmap_stream_lookup::find(decode_budget, bytes, object_number)?;
    decode_cmap_stream(decode_budget, dict, data)
}

/// CMap の既存 Filter 方針を維持し、未圧縮 payload は借用する。
fn decode_cmap_stream<'a>(
    budget: &DecodeBudget,
    dict: &[u8],
    data: &'a [u8],
) -> Option<Cow<'a, [u8]>> {
    if dict
        .windows(b"FlateDecode".len())
        .any(|w| w == b"FlateDecode")
    {
        decode_zlib(data, budget).map(Cow::Owned)
    } else {
        Some(Cow::Borrowed(data))
    }
}

/// CMap lookup 用に hex key を偶数桁の大文字へ正規化する。
#[cfg(test)]
fn normalize_hex_key(hex: &str) -> String {
    let mut key = hex.to_ascii_uppercase();
    if !key.len().is_multiple_of(2) {
        key.push('0');
    }
    key
}

/// PDF バイト列から stream payload を除いた bytes を返す。
pub(crate) fn pdf_bytes_without_stream_payloads(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> Vec<u8> {
    if let Some(index) = decode_budget.stream_index(bytes) {
        return index.without_payloads(decode_budget, bytes);
    }
    crate::metadata_input::legacy_copy(decode_budget, bytes)
}

/// PDF バイト列から Image XObject 以外の stream payload を抽出する。
fn extract_non_image_stream_payloads(decode_budget: &DecodeBudget, bytes: &[u8]) -> Vec<Vec<u8>> {
    if decode_budget.is_stopped() {
        return Vec::new();
    }
    if decode_budget
        .stream_index(bytes)
        .is_some_and(|index| index.has_xref())
    {
        let pages = crate::parser::extract_page_content_streams_with_page(decode_budget, bytes);
        let Some(mut streams) = decode_budget.metadata_vector(pages.len()) else {
            return Vec::new();
        };
        for (_, data) in pages {
            streams.push(data);
        }
        return streams;
    }
    let mut streams = Vec::new();
    if let Some(index) = decode_budget.stream_index(bytes) {
        for stream in index.streams(bytes) {
            if !crate::parser::is_image_dictionary(stream.dictionary)
                && let Some(decoded) =
                    decode_text_stream(decode_budget, stream.dictionary, stream.payload)
                && crate::metadata_workspace::try_push(decode_budget, &mut streams, decoded)
                    .is_none()
            {
                return Vec::new();
            }
            if decode_budget.is_stopped() {
                return Vec::new();
            }
        }
        return streams;
    }
    let mut cursor = 0usize;
    while let Some(stream_rel) = find_bytes(&bytes[cursor..], b"stream") {
        let stream_pos = cursor + stream_rel;
        let dict_start = stream_dictionary_start_before_stream(bytes, stream_pos);
        let dict = dict_start
            .map(|start| &bytes[start..stream_pos])
            .unwrap_or_default();
        let Some((data, next)) = crate::legacy_stream::payload(bytes, dict, stream_pos) else {
            break;
        };
        if dict.windows(b"/Subtype".len()).any(|w| w == b"/Subtype")
            && dict.windows(b"/Image".len()).any(|w| w == b"/Image")
        {
            cursor = next;
            continue;
        }
        if let Some(decoded) = decode_text_stream(decode_budget, dict, data)
            && crate::metadata_workspace::try_push(decode_budget, &mut streams, decoded).is_none()
        {
            return Vec::new();
        }
        if decode_budget.is_stopped() {
            return Vec::new();
        }
        cursor = next;
    }
    streams
}

/// PDF バイト列から `/Filter /FlateDecode` stream の展開済み payload を抽出する。
#[cfg(test)]
fn extract_flate_stream_payloads(decode_budget: &DecodeBudget, bytes: &[u8]) -> Vec<Vec<u8>> {
    extract_non_image_stream_payloads(decode_budget, bytes)
}

/// stream 直前の object dictionary 開始位置を返す。
fn stream_dictionary_start_before_stream(bytes: &[u8], stream_pos: usize) -> Option<usize> {
    let object_body_start = bytes[..stream_pos]
        .windows(b"obj".len())
        .rposition(|window| window == b"obj")
        .map(|position| position + b"obj".len())
        .unwrap_or(0);
    bytes[object_body_start..stream_pos]
        .windows(2)
        .position(|window| window == b"<<")
        .map(|position| object_body_start + position)
}

/// byte slice 内で needle の開始位置を探す。
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// PDF バイト列全体から `Tj` / `TJ` の直前にある文字列 operand を順序通り抽出する。
#[allow(dead_code)]
#[cfg(test)]
fn extract_pdf_text_show_operands_in_order(bytes: &[u8]) -> Vec<String> {
    extract_pdf_text_show_operands_in_order_with_cmap(bytes, &ToUnicodeCMap::default())
}

/// PDF バイト列全体から `Tj` / `TJ` の文字列 operand を CMap 適用込みで抽出する。
#[cfg(test)]
fn extract_pdf_text_show_operands_in_order_with_cmap(
    bytes: &[u8],
    cmap: &ToUnicodeCMap,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                if let Some((joined, next)) = parse_pdf_tj_array_with_cmap(bytes, i, cmap) {
                    if !joined.is_empty() {
                        result.push(joined);
                    }
                    i = next;
                    continue;
                }
            }
            b'(' => {
                if let Some((literal, next)) =
                    parse_pdf_literal_string_with_tounicode(bytes, i, cmap)
                {
                    if next_is_text_show_operator(bytes, next, b"Tj") && !literal.is_empty() {
                        result.push(literal);
                    }
                    i = next;
                    continue;
                }
            }
            b'<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'<' {
                    i += 1;
                    continue;
                }
                if let Some((hex, next)) = parse_pdf_hex_string_with_cmap(bytes, i, cmap) {
                    if next_is_text_show_operator(bytes, next, b"Tj") && !hex.is_empty() {
                        result.push(hex);
                    }
                    i = next;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    result
}

/// 指定位置の後続 token が text showing operator か判定する。
#[cfg(test)]
fn next_is_text_show_operator(bytes: &[u8], start: usize, operator: &[u8]) -> bool {
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    bytes.get(i..i + operator.len()) == Some(operator)
}

/// PDF の `BT ... ET` テキストオブジェクト範囲から文字列リテラルを抽出する。
#[cfg(test)]
#[allow(dead_code)]
fn extract_pdf_literals_in_text_objects(bytes: &[u8]) -> Vec<String> {
    extract_pdf_literals_in_text_objects_with_cmap(bytes, &ToUnicodeCMap::default())
}

/// PDF の `BT ... ET` テキストオブジェクト範囲から CMap 適用込みで文字列を抽出する。
#[cfg(test)]
fn extract_pdf_literals_in_text_objects_with_cmap(
    bytes: &[u8],
    cmap: &ToUnicodeCMap,
) -> Vec<String> {
    let source = String::from_utf8_lossy(bytes);
    let mut result = Vec::new();
    let mut cursor = 0usize;

    while let Some(bt_pos) = source[cursor..].find("BT") {
        let bt_index = cursor + bt_pos;
        let search_from = bt_index + 2;
        let Some(et_pos) = source[search_from..].find("ET") else {
            break;
        };
        let et_index = search_from + et_pos;
        let text_object = &source[bt_index..et_index];
        result.extend(extract_pdf_text_fragments_in_order_with_cmap(
            text_object.as_bytes(),
            cmap,
        ));
        cursor = et_index + 2;
    }

    result
}

/// テキストオブジェクトから `(...)` と `<...>` を出現順で抽出する。
#[allow(dead_code)]
#[cfg(test)]
pub(crate) fn extract_pdf_text_fragments_in_order(bytes: &[u8]) -> Vec<String> {
    extract_pdf_text_fragments_in_order_with_cmap(bytes, &ToUnicodeCMap::default())
}

/// テキストオブジェクトから `(...)` と `<...>` を CMap 適用込みで出現順に抽出する。
#[cfg(test)]
pub(crate) fn extract_pdf_text_fragments_in_order_with_cmap(
    bytes: &[u8],
    cmap: &ToUnicodeCMap,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                if let Some((joined, next)) = parse_pdf_tj_array_with_cmap(bytes, i, cmap) {
                    if !joined.is_empty() {
                        result.push(joined);
                    }
                    i = next;
                    continue;
                }
            }
            b'(' => {
                if let Some((literal, next)) =
                    parse_pdf_literal_string_with_tounicode(bytes, i, cmap)
                {
                    if next_is_text_show_operator(bytes, next, b"Tj") && !literal.is_empty() {
                        result.push(literal);
                    }
                    i = next;
                    continue;
                }
            }
            b'<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'<' {
                    i += 1;
                    continue;
                }
                if let Some((hex, next)) = parse_pdf_hex_string_with_cmap(bytes, i, cmap) {
                    if !hex.is_empty() {
                        result.push(hex);
                    }
                    i = next;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    result
}

/// encoded glyphのbyte列をToUnicodeまたはFont encodingで復号する。
pub(crate) use operand::decode_bytes as decode_glyph_bytes;

/// 字句検証済みの文字描画operandを借用し、合成Tj本文とfragment配列を作らず復号する。
pub(crate) fn decode_text_show_operand(
    budget: &DecodeBudget,
    raw: &[u8],
    cmap: &ToUnicodeCMap,
    font_encodings: &dyn crate::metadata_map::Lookup<String>,
    font_cmaps: &dyn crate::metadata_map::Lookup<ToUnicodeCMap>,
    font: Option<&str>,
) -> String {
    let active_cmap = font.and_then(|font| font_cmaps.get(font)).unwrap_or(cmap);
    operand::decode(
        budget,
        raw,
        active_cmap,
        font.and_then(|font| font_encodings.get(font))
            .map(String::as_str),
    )
    .unwrap_or_default()
}

/// `start` 位置の `[...] TJ` 配列を解析し、文字断片を連結して返す。
#[allow(dead_code)]
#[cfg(test)]
pub(crate) fn parse_pdf_tj_array(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    parse_pdf_tj_array_with_cmap(bytes, start, &ToUnicodeCMap::default())
}

/// `start` 位置の `[...] TJ` 配列を CMap 適用込みで解析する。
#[cfg(test)]
fn parse_pdf_tj_array_with_cmap(
    bytes: &[u8],
    start: usize,
    cmap: &ToUnicodeCMap,
) -> Option<(String, usize)> {
    if start >= bytes.len() || bytes[start] != b'[' {
        return None;
    }

    let mut i = start + 1;
    let mut joined = String::new();
    let mut found_closing_bracket = false;

    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                let (literal, next) = parse_pdf_literal_string_with_tounicode(bytes, i, cmap)?;
                joined.push_str(&literal);
                i = next;
                continue;
            }
            b'<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'<' {
                    i += 1;
                    continue;
                }
                let (hex, next) = parse_pdf_hex_string_with_cmap(bytes, i, cmap)?;
                joined.push_str(&hex);
                i = next;
                continue;
            }
            b']' => {
                i += 1;
                found_closing_bracket = true;
                break;
            }
            _ => {
                i += 1;
            }
        }
    }

    // `[` で始まって `]` が閉じない場合は壊れた TJ 配列とみなし、
    // 残りの text object をノイズとしてスキップする。
    if !found_closing_bracket {
        return Some((String::new(), bytes.len()));
    }

    // `]` の直後に `TJ` がある場合のみ text 配列として扱う。
    let mut j = i;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if j + 1 >= bytes.len() || bytes[j] != b'T' || bytes[j + 1] != b'J' {
        return None;
    }

    Some((joined, i))
}

/// `start` 位置の `(...)` リテラルをデコードして返す。
#[allow(dead_code)]
#[cfg(test)]
pub(crate) fn parse_pdf_literal_string(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    let (raw, next) = parse_pdf_literal_bytes(bytes, start)?;
    let current = raw
        .into_iter()
        .filter(|b| (0x20..=0x7e).contains(b) || matches!(b, b'\n' | b'\r' | b'\t'))
        .map(char::from)
        .collect::<String>();
    Some((current, next))
}

/// `start` 位置の `(...)` リテラルを ToUnicode 適用込みでデコードして返す。
#[cfg(test)]
fn parse_pdf_literal_string_with_tounicode(
    bytes: &[u8],
    start: usize,
    cmap: &ToUnicodeCMap,
) -> Option<(String, usize)> {
    let (raw, next) = parse_pdf_literal_bytes(bytes, start)?;
    if cmap.is_empty() {
        return Some((String::from_utf8_lossy(&raw).into_owned(), next));
    }
    let hex = raw
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let decoded = decode_hex_with_tounicode_only(&hex, cmap)
        .unwrap_or_else(|| String::from_utf8_lossy(&raw).into_owned());
    Some((decoded, next))
}

/// `start` 位置の `(...)` リテラルを raw byte 列として返す。
#[cfg(test)]
fn parse_pdf_literal_bytes(bytes: &[u8], start: usize) -> Option<(Vec<u8>, usize)> {
    if start >= bytes.len() || bytes[start] != b'(' {
        return None;
    }
    let mut i = start + 1;
    let mut current = Vec::new();
    let mut escaped = false;
    let mut nested_depth = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;

        if escaped {
            let (decoded, consumed) = decode_pdf_escape_sequence(bytes, i - 1);
            if let Some(ch) = decoded {
                current.push(ch as u8);
            }
            i = i - 1 + consumed;
            escaped = false;
            continue;
        }

        if b == b'\\' {
            escaped = true;
            continue;
        }
        if b == b'(' {
            nested_depth += 1;
            current.push(b'(');
            continue;
        }
        if b == b')' {
            if nested_depth == 0 {
                return Some((current, i));
            }
            nested_depth -= 1;
            current.push(b')');
            continue;
        }
        current.push(b);
    }

    // 閉じ括弧が無い literal は壊れた入力として扱い、呼び出し側で回復させる。
    None
}

/// PDF 文字列の `\` エスケープ1つをデコードし、消費バイト数を返す。
///
/// - 標準エスケープ: `\n \r \t \b \f \( \) \\`
/// - オクタルエスケープ: `\ddd`（1〜3桁、0..7）
/// - 行継続: `\` + 改行（`\n` / `\r` / `\r\n`）は文字を出力しない
#[cfg(test)]
fn decode_pdf_escape_sequence(bytes: &[u8], escape_start: usize) -> (Option<char>, usize) {
    if escape_start >= bytes.len() {
        return (Some('\\'), 0);
    }
    let first = bytes[escape_start];
    match first {
        b'\n' => (None, 1),
        b'\r' => {
            if escape_start + 1 < bytes.len() && bytes[escape_start + 1] == b'\n' {
                (None, 2)
            } else {
                (None, 1)
            }
        }
        b'n' => (Some('\n'), 1),
        b'r' => (Some('\r'), 1),
        b't' => (Some('\t'), 1),
        b'b' => (Some('\u{0008}'), 1),
        b'f' => (Some('\u{000C}'), 1),
        b'(' => (Some('('), 1),
        b')' => (Some(')'), 1),
        b'\\' => (Some('\\'), 1),
        b'0'..=b'7' => {
            let mut value = (first - b'0') as u16;
            let mut consumed = 1usize;
            for _ in 0..2 {
                let idx = escape_start + consumed;
                if idx >= bytes.len() {
                    break;
                }
                let b = bytes[idx];
                if !(b'0'..=b'7').contains(&b) {
                    break;
                }
                value = value * 8 + (b - b'0') as u16;
                consumed += 1;
            }
            let ch = char::from_u32((value & 0xff) as u32).unwrap_or('?');
            (Some(ch), consumed)
        }
        _ => (Some(first as char), 1),
    }
}

/// `start` 位置の `<...>` hex をデコードして返す。
#[allow(dead_code)]
#[cfg(test)]
pub(crate) fn parse_pdf_hex_string(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    parse_pdf_hex_string_with_cmap(bytes, start, &ToUnicodeCMap::default())
}

/// `start` 位置の `<...>` hex を CMap 優先でデコードして返す。
#[cfg(test)]
fn parse_pdf_hex_string_with_cmap(
    bytes: &[u8],
    start: usize,
    cmap: &ToUnicodeCMap,
) -> Option<(String, usize)> {
    if start + 1 >= bytes.len() || bytes[start] != b'<' || bytes[start + 1] == b'<' {
        return None;
    }
    let mut i = start + 1;
    let mut hex_buf = String::new();
    let mut found_closing = false;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b == b'>' {
            found_closing = true;
            break;
        }
        if (b as char).is_ascii_hexdigit() {
            hex_buf.push(b as char);
        }
    }
    if !found_closing {
        return None;
    }
    if hex_buf.is_empty() {
        return Some((String::new(), i));
    }
    if !hex_buf.len().is_multiple_of(2) {
        hex_buf.push('0');
    }
    let decoded = decode_hex_with_cmap(&hex_buf, cmap).unwrap_or_default();
    Some((decoded, i))
}

/// `start` 位置の `<...>` hex を font encoding 優先でデコードして返す。
#[cfg(test)]
fn parse_pdf_hex_string_with_font_encoding(
    bytes: &[u8],
    start: usize,
    cmap: &ToUnicodeCMap,
    encoding: Option<&str>,
) -> Option<(String, usize)> {
    if start + 1 >= bytes.len() || bytes[start] != b'<' || bytes[start + 1] == b'<' {
        return None;
    }
    let mut i = start + 1;
    let mut hex_buf = String::new();
    let mut found_closing = false;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b == b'>' {
            found_closing = true;
            break;
        }
        if (b as char).is_ascii_hexdigit() {
            hex_buf.push(b as char);
        }
    }
    if !found_closing {
        return None;
    }
    if hex_buf.is_empty() {
        return Some((String::new(), i));
    }
    if !hex_buf.len().is_multiple_of(2) {
        hex_buf.push('0');
    }
    let decoded = decode_hex_with_tounicode_only(&hex_buf, cmap)
        .or_else(|| {
            encoding.and_then(|encoding| decode_hex_with_external_encoding(&hex_buf, encoding))
        })
        .or_else(|| decode_hex_text(&hex_buf))
        .unwrap_or_default();
    Some((decoded, i))
}

/// CMap を優先して hex text をデコードする。
#[cfg(test)]
fn decode_hex_with_cmap(hex: &str, cmap: &ToUnicodeCMap) -> Option<String> {
    if cmap.is_empty() {
        return decode_hex_text(hex);
    }
    let key = normalize_hex_key(hex);
    if cmap.key_is_allowed(&key)
        && let Some(mapped) = cmap.mappings.get(&key)
    {
        return Some(mapped.clone());
    }
    if let Some(decoded) = decode_hex_with_cmap_key_widths(&key, cmap) {
        return Some(decoded);
    }
    decode_hex_text(hex)
}

/// ToUnicode CMap による明示 mapping だけで hex text を復号する。
#[cfg(test)]
fn decode_hex_with_tounicode_only(hex: &str, cmap: &ToUnicodeCMap) -> Option<String> {
    if cmap.is_empty() {
        return None;
    }
    let key = normalize_hex_key(hex);
    if cmap.key_is_allowed(&key)
        && let Some(mapped) = cmap.mappings.get(&key)
    {
        return Some(mapped.clone());
    }
    decode_hex_with_cmap_key_widths(&key, cmap)
}

/// PDF standard CMap 名に対応する外部 encoding として hex text を復号する。
#[cfg(test)]
fn decode_hex_with_external_encoding(hex: &str, encoding: &str) -> Option<String> {
    if matches!(
        encoding,
        "UniGB-UTF16-H"
            | "UniGB-UCS2-H"
            | "UniCNS-UTF16-H"
            | "UniCNS-UCS2-H"
            | "UniKS-UTF16-H"
            | "UniKS-UCS2-H"
    ) {
        return decode_hex_utf16be_without_bom(hex);
    }

    let bytes = hex_to_bytes(hex)?;
    let decoded = match encoding {
        "GBK-EUC-H" | "GBK2K-H" => GBK.decode(&bytes).0.into_owned(),
        "ETen-B5-H" => BIG5.decode(&bytes).0.into_owned(),
        "KSCms-UHC-H" => EUC_KR.decode(&bytes).0.into_owned(),
        _ => return None,
    };
    Some(decoded)
}

/// CMap に登録された CID hex key 幅を使って hex text を分割して復号する。
#[cfg(test)]
fn decode_hex_with_cmap_key_widths(hex: &str, cmap: &ToUnicodeCMap) -> Option<String> {
    let mut widths = if cmap.code_space_ranges.is_empty() {
        cmap.mappings
            .keys()
            .map(|key| key.len())
            .filter(|width| *width > 0)
            .collect::<Vec<_>>()
    } else {
        cmap.code_space_widths()
    };
    widths.extend(
        cmap.mappings
            .keys()
            .filter(|key| cmap.key_is_allowed(key))
            .map(|key| key.len())
            .filter(|width| *width > 0),
    );
    widths.sort_unstable_by(|a, b| b.cmp(a));
    widths.dedup();

    let mut decoded = String::new();
    let mut idx = 0usize;
    while idx < hex.len() {
        let mut matched = false;
        for width in &widths {
            if idx + *width > hex.len() {
                continue;
            }
            let chunk = &hex[idx..idx + *width];
            if cmap.key_is_allowed(chunk)
                && let Some(mapped) = cmap.mappings.get(chunk)
            {
                decoded.push_str(mapped);
                idx += *width;
                matched = true;
                break;
            }
        }
        if !matched {
            return None;
        }
    }
    Some(decoded)
}

/// 16進文字列をテキストへ変換する。
///
/// `FEFF` で始まる場合は UTF-16BE とみなし、それ以外は Shift_JIS/ASCII として扱う。
#[cfg(test)]
fn decode_hex_text(hex: &str) -> Option<String> {
    if hex.len() >= 4 && hex[..4].eq_ignore_ascii_case("FEFF") {
        return decode_hex_utf16be(hex);
    }
    decode_hex_shift_jis(hex).or_else(|| decode_hex_ascii(hex))
}

/// 16進文字列を Shift_JIS として UTF-8 `String` へ変換する。
#[cfg(test)]
fn decode_hex_shift_jis(hex: &str) -> Option<String> {
    let bytes = hex_to_bytes(hex)?;
    let (decoded, _, _) = SHIFT_JIS.decode(&bytes);
    Some(decoded.into_owned())
}

/// 偶数桁 hex を byte 列へ変換する。
#[cfg(test)]
fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut idx = 0usize;
    while idx + 1 < hex.len() {
        let pair = &hex[idx..idx + 2];
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
        idx += 2;
    }
    Some(bytes)
}

/// literal/hexのencoded byte全容量を確保前に予約し、改行を正規化する。
pub(crate) fn pdf_string_bytes(budget: &DecodeBudget, raw: &[u8]) -> Option<Vec<u8>> {
    if budget.is_stopped() {
        return None;
    }
    let mut length = 0usize;
    crate::pdf_binary_string::visit_string(raw, |_| {
        length = length.checked_add(1)?;
        Some(())
    })?;
    let mut bytes = budget.metadata_vector(length)?;
    crate::pdf_binary_string::visit_string(raw, |byte| {
        bytes.push(byte);
        Some(())
    })?;
    Some(bytes)
}

/// 16進文字列を ASCII 文字列へ変換する。
#[cfg(test)]
fn decode_hex_ascii(hex: &str) -> Option<String> {
    let mut out = String::new();
    let mut idx = 0usize;
    while idx + 1 < hex.len() {
        let pair = &hex[idx..idx + 2];
        let value = u8::from_str_radix(pair, 16).ok()?;
        if (0x20..=0x7e).contains(&value) || value == b'\n' || value == b'\r' || value == b'\t' {
            out.push(value as char);
        }
        idx += 2;
    }
    Some(out)
}

/// UTF-16BE（BOM付き）hex文字列を UTF-8 `String` へ変換する。
#[cfg(test)]
fn decode_hex_utf16be(hex: &str) -> Option<String> {
    let mut text = decode_hex_utf16be_without_bom(hex)?;
    if text.starts_with('\u{feff}') {
        text.remove(0);
    }
    Some(text)
}

/// BOM が無い UTF-16BE hex も UTF-8 `String` へ変換する。
#[cfg(test)]
fn decode_hex_utf16be_without_bom(hex: &str) -> Option<String> {
    let mut units = Vec::new();
    let mut idx = 0usize;
    while idx + 3 < hex.len() {
        let chunk = &hex[idx..idx + 4];
        let unit = u16::from_str_radix(chunk, 16).ok()?;
        units.push(unit);
        idx += 4;
    }

    if units.is_empty() {
        return Some(String::new());
    }

    String::from_utf16(&units).ok()
}

/// 抽出した文字列が人間可読テキストとして妥当か判定する。
fn is_readable_text_candidate(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() < 2 {
        return false;
    }

    let meaningful_count = trimmed
        .chars()
        .filter(|c| c.is_alphanumeric() || !c.is_ascii())
        .count();
    let control_count = trimmed
        .chars()
        .filter(|c| {
            c.is_control()
                && *c != '\n'
                && *c != '\r'
                && *c != '\t'
                && *c != '\u{0008}'
                && *c != '\u{000C}'
        })
        .count();
    meaningful_count >= 1 && control_count == 0
}

#[cfg(test)]
mod tests {
    use super::{extract_flate_stream_payloads, extract_pdf_text_fragments_in_order};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn tj_array_with_numeric_kerning_keeps_text_order(
            left in "[A-Za-z]{1,8}",
            right in "[A-Za-z]{1,8}",
            kerning in -5000i32..5000i32
        ) {
            let text_obj = format!("BT [({left}) {kerning} ({right})] TJ ET");
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(&fragments[0], &format!("{left}{right}"));
        }

        #[test]
        fn tj_array_with_decimal_kerning_keeps_text_order(
            left in "[A-Za-z]{1,8}",
            right in "[A-Za-z]{1,8}",
            int_part in -5000i32..5000i32,
            frac_part in 0u16..1000u16
        ) {
            let text_obj = format!("BT [({left}) {}.{} ({right})] TJ ET", int_part, frac_part);
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(&fragments[0], &format!("{left}{right}"));
        }

        #[test]
        fn escaped_parentheses_roundtrip_in_literal(
            left in "[A-Za-z]{1,8}",
            right in "[A-Za-z]{1,8}"
        ) {
            let text_obj = format!("BT ({left}\\({right}\\)) Tj ET");
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(&fragments[0], &format!("{left}({right})"));
        }

        #[test]
        fn escaped_backslash_roundtrip_in_literal(
            left in "[A-Za-z]{1,8}",
            right in "[A-Za-z]{1,8}"
        ) {
            let text_obj = format!("BT ({left}\\\\{right}) Tj ET");
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(&fragments[0], &format!("{left}\\{right}"));
        }

        #[test]
        fn nested_parentheses_roundtrip_in_literal(
            left in "[A-Za-z]{1,6}",
            inner in "[A-Za-z]{1,6}",
            right in "[A-Za-z]{1,6}"
        ) {
            let text_obj = format!("BT ({left}({inner}){right}) Tj ET");
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(&fragments[0], &format!("{left}({inner}){right}"));
        }

        #[test]
        fn line_continuation_escape_roundtrip(
            a in "[A-Za-z]{1,4}",
            b in "[A-Za-z]{1,4}",
            c in "[A-Za-z]{1,4}"
        ) {
            let variants = ["\\\n", "\\\r", "\\\r\n"];
            for v1 in variants {
                for v2 in variants {
                    let text_obj = format!("BT ({a}{v1}{b}{v2}{c}) Tj ET");
                    let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
                    prop_assert_eq!(fragments.len(), 1);
                    prop_assert_eq!(&fragments[0], &format!("{a}{b}{c}"));
                }
            }
        }

        #[test]
        fn tj_mixed_literal_and_hex_roundtrip(
            left in "[A-Za-z]{1,6}",
            middle in "[A-Za-z]{1,6}",
            right in "[A-Za-z]{1,6}",
            kerning in -3000i32..3000i32
        ) {
            let middle_hex = ascii_to_hex(&middle);
            let text_obj = format!(
                "BT [({left}) {kerning} <{middle_hex}> {kerning} ({right})] TJ ET"
            );
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(&fragments[0], &format!("{left}{middle}{right}"));
        }

        #[test]
        fn tj_mixed_utf16_hex_and_literal_roundtrip(
            left in "[A-Za-z]{1,6}",
            right in "[A-Za-z]{1,6}",
            kerning in -3000i32..3000i32
        ) {
            let text_obj = format!(
                "BT [({left}) {kerning} <FEFF65E5672C> {kerning} ({right})] TJ ET"
            );
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(&fragments[0], &format!("{left}日本{right}"));
        }

        #[test]
        fn tj_mixed_escaped_octal_and_hex_roundtrip(
            left in "[A-Za-z]{1,4}",
            escaped_mid in "[A-Za-z]{1,4}",
            octal_mid in "[A-Za-z]{1,4}",
            hex_mid in "[A-Za-z]{1,4}",
            right in "[A-Za-z]{1,4}",
            kerning in -3000i32..3000i32
        ) {
            let hex = ascii_to_hex(&hex_mid);
            let text_obj = format!(
                "BT [({left}\\({escaped_mid}\\)\\050{octal_mid}\\051) {kerning} <{hex}> {kerning} ({right})] TJ ET"
            );
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(
                &fragments[0],
                &format!("{left}({escaped_mid})({octal_mid}){hex_mid}{right}")
            );
        }

        #[test]
        fn tj_literal_line_continuation_and_hex_roundtrip(
            left in "[A-Za-z]{1,4}",
            a in "[A-Za-z]{1,4}",
            b in "[A-Za-z]{1,4}",
            right in "[A-Za-z]{1,4}",
            kerning in -3000i32..3000i32
        ) {
            let variants = ["\\\n", "\\\r", "\\\r\n"];
            let hex = ascii_to_hex(&b);
            for cont in variants {
                let text_obj = format!(
                    "BT [({left}{a}{cont}) {kerning} <{hex}> {kerning} ({right})] TJ ET"
                );
                let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
                prop_assert_eq!(fragments.len(), 1);
                prop_assert_eq!(&fragments[0], &format!("{left}{a}{b}{right}"));
            }
        }

        #[test]
        fn tj_array_ignores_exponent_and_signed_numeric_tokens(
            left in "[A-Za-z]{1,4}",
            middle in "[A-Za-z]{1,4}",
            right in "[A-Za-z]{1,4}"
        ) {
            let numeric_tokens = ["-120", "+45.5", "-0.125", "1e2", "-3.4E-2", "+5E+1"];
            for token in numeric_tokens {
                let text_obj = format!("BT [({left}) {token} ({middle}) {token} ({right})] TJ ET");
                let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
                prop_assert_eq!(fragments.len(), 1);
                prop_assert_eq!(&fragments[0], &format!("{left}{middle}{right}"));
            }
        }

        #[test]
        fn tj_array_with_utf16_hex_ignores_exponent_numeric_tokens(
            left in "[A-Za-z]{1,4}",
            right in "[A-Za-z]{1,4}"
        ) {
            let numeric_tokens = ["1e2", "-3.4E-2", "+5E+1"];
            for token in numeric_tokens {
                let text_obj = format!(
                    "BT [({left}) {token} <FEFF65E5672C> {token} ({right})] TJ ET"
                );
                let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
                prop_assert_eq!(fragments.len(), 1);
                prop_assert_eq!(&fragments[0], &format!("{left}日本{right}"));
            }
        }

        #[test]
        fn tj_array_with_nested_parentheses_literal_roundtrip(
            left in "[A-Za-z]{1,4}",
            inner in "[A-Za-z]{1,4}",
            right in "[A-Za-z]{1,4}",
            tail in "[A-Za-z]{1,4}",
            kerning in -3000i32..3000i32
        ) {
            let text_obj = format!(
                "BT [({left}({inner}){right}) {kerning} <20544444> {kerning} ({tail})] TJ ET"
            );
            let fragments = extract_pdf_text_fragments_in_order(text_obj.as_bytes());
            prop_assert_eq!(fragments.len(), 1);
            prop_assert_eq!(&fragments[0], &format!("{left}({inner}){right} TDD{tail}"));
        }
    }

    #[test]
    fn flate_payload_extraction_skips_image_xobject_streams() {
        let content = flate2_compress(b"BT (text) Tj ET");
        let image = flate2_compress(&[255, 0, 0]);
        let mut pdf = Vec::new();
        pdf.extend_from_slice(
            format!(
                "1 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
                content.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&content);
        pdf.extend_from_slice(b"\nendstream endobj\n");
        pdf.extend_from_slice(
            format!(
                "2 0 obj << /Subtype /Image /Width 1 /Height 1 /Length {} /Filter /FlateDecode >> stream\n",
                image.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&image);
        pdf.extend_from_slice(b"\nendstream endobj\n");

        let streams = extract_flate_stream_payloads(&Default::default(), &pdf);

        assert_eq!(streams, vec![b"BT (text) Tj ET".to_vec()]);
    }

    fn flate2_compress(bytes: &[u8]) -> Vec<u8> {
        use flate2::{Compression, write::ZlibEncoder};
        use std::io::Write;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn unclosed_literal_does_not_swallow_following_literal() {
        let text_obj = b"BT (Broken Tj (Recovered) Tj ET";
        let fragments = extract_pdf_text_fragments_in_order(text_obj);
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0], "Recovered");
    }

    #[test]
    fn unclosed_hex_does_not_swallow_following_literal() {
        let text_obj = b"BT <48656C6C6F Tj (Recovered) Tj ET";
        let fragments = extract_pdf_text_fragments_in_order(text_obj);
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0], "Recovered");
    }

    #[test]
    fn text_show_operand_scan_does_not_treat_eten_name_as_end_text() {
        let bytes = b"(UniCNS-UCS2-H) Tj\n(ETen-B5-H) Tj\n(KSCms-UHC-H) Tj";
        let fragments = super::extract_pdf_text_show_operands_in_order(bytes);
        assert_eq!(
            fragments,
            vec![
                "UniCNS-UCS2-H".to_string(),
                "ETen-B5-H".to_string(),
                "KSCms-UHC-H".to_string()
            ]
        );
    }

    /// ASCII 文字列を PDF hex 文字列表現へ変換する（テスト用）。
    fn ascii_to_hex(text: &str) -> String {
        text.as_bytes().iter().map(|b| format!("{b:02X}")).collect()
    }
}

/// 本文・CMap stream の既存 Filter 方針で、検査済み payload を展開する。
fn decode_text_stream(decode_budget: &DecodeBudget, dict: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if dict
        .windows(b"FlateDecode".len())
        .any(|window| window == b"FlateDecode")
    {
        decode_zlib(data, decode_budget)
    } else {
        decode_budget.copy_stream_bytes(data)
    }
}
