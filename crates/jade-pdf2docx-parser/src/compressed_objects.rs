//! ObjStmの選択・owner索引・本文保存を共有metadata予算で制限する。
use super::{
    DecodeBudget, XrefStreamEntry, decode_content_stream_payload, extract_stream_object_at_offset,
    parse_ascii_u32, parse_pdf_u32_after_name, xref_offset_entries,
};
use crate::object_bytes::{BodyEntry, CompressedObjects};
use std::{cmp::Reverse, rc::Rc};

/// 旧xrefの番号・entry・入力順を保持し、最後の更新だけを選ぶ。
type LegacyEntry = (u32, XrefStreamEntry, usize);
/// owner・object番号・member位置をまとめ、ownerごとに一回だけ展開する。
type Member = (u32, u32, u32);
/// member位置と、headerが有効な場合のobject番号・借用本文。
type Candidate<'a> = (u32, Option<(u32, &'a [u8])>);

/// 同じ検査済み入力では完成本文を共有し、失敗・停止の結果は保存しない。
pub(super) fn extract(budget: &DecodeBudget, bytes: &[u8]) -> CompressedObjects {
    if budget.is_stopped() {
        return CompressedObjects::default();
    }
    if let Some(objects) = budget.compressed_objects(bytes) {
        return objects;
    }
    let Some(objects) = decode(budget, bytes) else {
        return CompressedObjects::default();
    };
    budget.cache_compressed_objects(bytes, objects.clone());
    objects
}

/// 正式xrefのmemberを借用し、旧xrefは全候補を予約して重複を除く。
fn decode(budget: &DecodeBudget, bytes: &[u8]) -> Option<CompressedObjects> {
    if let Some(index) = budget.stream_index(bytes)
        && let Some(entries) = index.compressed()
    {
        let mut members = budget.metadata_vector(entries.count())?;
        members.extend(
            index
                .compressed()?
                .map(|(number, owner, member)| (owner, number, member)),
        );
        return decode_members(budget, members, |owner| index.for_object(bytes, owner));
    }
    let mut entries = Vec::<LegacyEntry>::new();
    xref_offset_entries::visit_streams(budget, bytes, |number, entry| {
        let order = entries.len();
        crate::metadata_workspace::try_push(budget, &mut entries, (number, entry, order))
    })?;
    if budget.is_stopped() {
        return None;
    }
    entries.sort_unstable_by_key(|&(number, _, order)| (number, Reverse(order)));
    entries.dedup_by_key(|entry| entry.0);
    let count = entries
        .iter()
        .filter(|(_, entry, _)| matches!(entry, XrefStreamEntry::Compressed { .. }))
        .count();
    let mut members = budget.metadata_vector(count)?;
    for &(number, entry, _) in &entries {
        if let XrefStreamEntry::Compressed {
            object_stream,
            index,
        } = entry
        {
            members.push((object_stream, number, index));
        }
    }
    decode_members(budget, members, |owner| {
        let position = entries.binary_search_by_key(&owner, |entry| entry.0).ok()?;
        let XrefStreamEntry::Uncompressed { offset } = entries[position].1 else {
            return None;
        };
        extract_stream_object_at_offset(budget, bytes, offset)
    })
}

/// owner索引を追加mapなしで整列し、保存slotと本文の全容量を確保前に予約する。
fn decode_members<'a>(
    budget: &DecodeBudget,
    mut members: Vec<Member>,
    mut stream: impl FnMut(u32) -> Option<(&'a [u8], &'a [u8])>,
) -> Option<CompressedObjects> {
    members.sort_unstable_by_key(|&(owner, number, member)| (owner, member, number));
    let mut bodies = budget.metadata_vector::<BodyEntry>(members.len())?;
    let mut cursor = 0;
    while cursor < members.len() {
        let owner = members[cursor].0;
        let end = cursor + members[cursor..].partition_point(|entry| entry.0 == owner);
        if let Some((dict, data)) = stream(owner)
            && crate::metadata::has_type(dict, b"ObjStm")
            && let Some(payload) = decode_content_stream_payload(budget, dict, data)
            && let Some(candidates) = member_bodies(dict, &payload)
        {
            let mut candidates = candidates.peekable();
            for &(_, number, member) in &members[cursor..end] {
                while candidates.peek().is_some_and(|&(index, _)| index < member) {
                    candidates.next();
                }
                if let Some(&(index, Some((found, body)))) = candidates.peek()
                    && index == member
                    && found == number
                {
                    budget.charge_metadata_workspace_bytes(
                        (std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>()) as u64,
                    )?;
                    let mut stored = budget.metadata_vector(body.len())?;
                    stored.extend_from_slice(body);
                    bodies.push((number, Rc::new(stored)));
                }
            }
        }
        if budget.is_stopped() {
            return None;
        }
        cursor = end;
    }
    CompressedObjects::from_entries(budget, bodies)
}

/// ObjStmのheaderを一回ずつ借用検査し、配列を作らずmember位置順の本文を返す。
fn member_bodies<'a>(
    dict: &[u8],
    payload: &'a [u8],
) -> Option<impl Iterator<Item = Candidate<'a>>> {
    let first = usize::try_from(parse_pdf_u32_after_name(dict, b"/First")?).ok()?;
    let n = usize::try_from(parse_pdf_u32_after_name(dict, b"/N")?).ok()?;
    let header = payload.get(..first)?;
    let area = payload.get(first..)?;
    let mut tokens = header
        .split(|byte| byte.is_ascii_whitespace() || matches!(byte, b'[' | b']'))
        .filter(|token| !token.is_empty());
    if tokens.clone().count() < n.checked_mul(2)? {
        return None;
    }
    Some((0..n).map(move |index| {
        let number = tokens.next().and_then(parse_ascii_u32);
        let start = tokens
            .next()
            .and_then(parse_ascii_u32)
            .and_then(|n| usize::try_from(n).ok());
        let end = if index + 1 < n {
            let mut next = tokens.clone();
            next.next();
            next.next()
                .and_then(parse_ascii_u32)
                .and_then(|n| usize::try_from(n).ok())
        } else {
            Some(area.len())
        };
        let body = number
            .zip(start)
            .zip(end)
            .and_then(|((number, start), end)| area.get(start..end).map(|body| (number, body)));
        (index as u32, body)
    }))
}
