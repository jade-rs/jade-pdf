//! metadata本文のコピー・連結・UTF-8置換を確保前の共有予算で制限する。
use crate::{decode::DecodeBudget, lossy_chunks::LossyChunks};
use std::borrow::Cow;

/// 有効UTF-8は借用し、不正入力は全置換容量を予約してからfallible配列へ保存する。
pub(crate) fn text<'a>(budget: &DecodeBudget, bytes: &'a [u8]) -> Option<Cow<'a, str>> {
    if budget.is_stopped() {
        return None;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(Cow::Borrowed(text));
    }
    let length = LossyChunks::new(bytes).fold(0usize, |n, chunk| n.saturating_add(chunk.len()));
    let mut output = budget.metadata_vector(length)?;
    for chunk in LossyChunks::new(bytes) {
        output.extend_from_slice(chunk.as_bytes());
    }
    String::from_utf8(output).ok().map(Cow::Owned)
}

/// 旧経路の本文除去を、入力と置換改行の借用だけで列挙する。
fn legacy_parts(bytes: &[u8]) -> impl Iterator<Item = &[u8]> + Clone {
    let mut cursor = 0;
    let mut separator = false;
    let mut finished = false;
    std::iter::from_fn(move || {
        if separator {
            separator = false;
            return Some(&b"\nendstream"[..]);
        }
        if finished {
            return None;
        }
        if let Some(relative) = bytes[cursor..].windows(6).position(|w| w == b"stream") {
            let start = cursor + relative;
            if let Some(end) = bytes[start..].windows(9).position(|w| w == b"endstream") {
                let part = &bytes[cursor..start + 6];
                cursor = start + end + 9;
                separator = true;
                return Some(part);
            }
        }
        finished = true;
        Some(&bytes[cursor..])
    })
}

/// 本文除去後の全出力容量を先に数え、一度だけコピー配列を確保する。
pub(crate) fn legacy_copy(budget: &DecodeBudget, bytes: &[u8]) -> Vec<u8> {
    if budget.is_stopped() {
        return Vec::new();
    }
    let parts = legacy_parts(bytes);
    let length = parts
        .clone()
        .fold(0usize, |n, part| n.saturating_add(part.len()));
    let Some(mut output) = budget.metadata_vector(length) else {
        return Vec::new();
    };
    for part in parts {
        output.extend_from_slice(part);
    }
    output
}

/// 連結前に新配列の全容量を予約し、既存本文を複製せず移動する。
pub(crate) fn extend_capacity(
    budget: &DecodeBudget,
    target: &mut Vec<u8>,
    additional: usize,
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    if additional == 0 {
        return Some(());
    }
    let mut replacement = budget.metadata_vector(target.len().saturating_add(additional))?;
    replacement.append(target);
    *target = replacement;
    Some(())
}
