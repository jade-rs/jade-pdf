//! 旧xrefのentryを借用走査し、保存先の容量制限を迂回しない。
use super::{
    DecodeBudget, XrefStreamEntry, decode_zlib, find_bytes, next_pdf_line, parse_ascii_u32,
    read_be_uint, stream_dictionary_before_stream, trim_ascii,
};
use std::borrow::Cow;

/// 最初のxref tableをtoken配列なしで読み、in-use entryだけを通知する。
pub(super) fn visit_table(
    bytes: &[u8],
    mut visitor: impl FnMut(u32, usize) -> Option<()>,
) -> Option<()> {
    let Some(xref_pos) = find_bytes(bytes, b"xref") else {
        return Some(());
    };
    let mut cursor = xref_pos + 4;
    let mut current_object = None;
    let mut remaining = 0;
    while cursor < bytes.len() {
        let (line, next) = next_pdf_line(bytes, cursor);
        cursor = next;
        let line = trim_ascii(line);
        if line.starts_with(b"trailer") || line.starts_with(b"startxref") {
            break;
        }
        let mut tokens = line
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|token| !token.is_empty());
        let (Some(first), Some(second)) = (tokens.next(), tokens.next()) else {
            continue;
        };
        let third = tokens.next();
        if third.is_none() {
            if let (Some(start), Some(count)) = (parse_ascii_u32(first), parse_ascii_u32(second)) {
                current_object = start.checked_add(count.saturating_sub(1)).map(|_| start);
                remaining = count;
            }
        } else if remaining > 0 {
            if third == Some(b"n".as_slice())
                && let Some((number, offset)) = current_object.zip(parse_ascii_u32(first))
                && offset > 0
            {
                visitor(number, offset as usize)?;
            }
            remaining -= 1;
            current_object = current_object.and_then(|number| number.checked_add(1));
        }
    }
    Some(())
}

/// xref streamを検査済み範囲または旧走査から読み、保存先へentryを通知する。
pub(super) fn visit_streams(
    budget: &DecodeBudget,
    bytes: &[u8],
    mut visitor: impl FnMut(u32, XrefStreamEntry) -> Option<()>,
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let mut read = |dict: &[u8], data: &[u8]| {
        if crate::metadata::has_type(dict, b"XRef") {
            visit_payload(budget, dict, data, &mut visitor);
        }
        (!budget.is_stopped()).then_some(())
    };
    if let Some(index) = budget.stream_index(bytes) {
        for stream in index.streams(bytes) {
            read(stream.dictionary, stream.payload)?;
        }
        return Some(());
    }
    let mut cursor = 0;
    while let Some(relative) = find_bytes(&bytes[cursor..], b"stream") {
        let start = cursor + relative;
        let Some(dict) = stream_dictionary_before_stream(bytes, start) else {
            cursor = start + 6;
            continue;
        };
        let Some(end_relative) = find_bytes(&bytes[start..], b"endstream") else {
            break;
        };
        let mut data_start = start + 6;
        if bytes.get(data_start) == Some(&b'\r') {
            data_start += 1;
        }
        if bytes.get(data_start) == Some(&b'\n') {
            data_start += 1;
        }
        let mut end = start + end_relative;
        while end > data_start && matches!(bytes.get(end - 1), Some(b'\r') | Some(b'\n')) {
            end -= 1;
        }
        if let Some(data) = bytes.get(data_start..end) {
            read(dict, data)?;
        }
        cursor = start + end_relative + 9;
    }
    Some(())
}

/// 未圧縮本文を借用し、不正なstream全体を除外してからentryを保存先へ渡す。
pub(super) fn visit_payload(
    budget: &DecodeBudget,
    dict: &[u8],
    data: &[u8],
    mut visitor: impl FnMut(u32, XrefStreamEntry) -> Option<()>,
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let mut widths = crate::metadata::array_entries(crate::metadata::value(dict, b"W")?, 4)?;
    let mut fields = [0; 3];
    for width in &mut fields {
        *width = usize::try_from(parse_ascii_u32(trim_ascii(widths.next()??))?).ok()?;
        if *width > 8 {
            return None;
        }
    }
    if widths.next().is_some() {
        return None;
    }
    let length = fields
        .iter()
        .try_fold(0usize, |sum, width| sum.checked_add(*width))?;
    if length == 0 {
        return None;
    }
    let payload = if dict
        .windows(b"FlateDecode".len())
        .any(|window| window == b"FlateDecode")
    {
        Cow::Owned(decode_zlib(data, budget)?)
    } else {
        Cow::Borrowed(data)
    };
    walk_payload(dict, &payload, fields, length, |_, _| Some(()))?;
    walk_payload(dict, &payload, fields, length, &mut visitor)
}

/// Indexを借用して再走査し、範囲と本文長を検査してからentryを読む。
fn walk_payload(
    dict: &[u8],
    payload: &[u8],
    fields: [usize; 3],
    length: usize,
    mut visitor: impl FnMut(u32, XrefStreamEntry) -> Option<()>,
) -> Option<()> {
    let mut cursor = 0usize;
    let mut range = |start: u32, count: u32| {
        start.checked_add(count.saturating_sub(1))?;
        let end = cursor.checked_add(usize::try_from(count).ok()?.checked_mul(length)?)?;
        if end > payload.len() {
            return None;
        }
        for position in 0..count {
            let entry = &payload[cursor..cursor + length];
            cursor += length;
            let kind = if fields[0] == 0 {
                1
            } else {
                read_be_uint(&entry[..fields[0]])?
            };
            let first_end = fields[0] + fields[1];
            let first = read_be_uint(&entry[fields[0]..first_end])?;
            let second = read_be_uint(&entry[first_end..])?;
            let value = match kind {
                1 if first > 0 => Some(XrefStreamEntry::Uncompressed {
                    offset: usize::try_from(first).ok()?,
                }),
                2 => Some(XrefStreamEntry::Compressed {
                    object_stream: u32::try_from(first).ok()?,
                    index: u32::try_from(second).ok()?,
                }),
                _ => None,
            };
            if let Some(value) = value {
                visitor(start + position, value)?;
            }
        }
        Some(())
    };
    if let Some(index) = crate::metadata::value(dict, b"Index") {
        let mut values = crate::metadata::array_entries(index, usize::MAX)?;
        while let Some(start) = values.next() {
            let start = parse_ascii_u32(trim_ascii(start?))?;
            let count = parse_ascii_u32(trim_ascii(values.next()??))?;
            range(start, count)?;
        }
    } else {
        range(
            0,
            parse_ascii_u32(trim_ascii(crate::metadata::value(dict, b"Size")?))?,
        )?;
    }
    Some(())
}
