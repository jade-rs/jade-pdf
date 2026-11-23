//! Adobe glyph名からUnicodeを取り出し、未知のcomponentを確認不能として区別する。
use crate::decode::DecodeBudget;

#[path = "glyph_names_data.rs"]
mod tables;

/// Unicodeを順に通知し、未知componentの有無とキャンセルを区別する。
pub(crate) fn visit(
    budget: &DecodeBudget,
    name: &[u8],
    zapf: bool,
    mut emit: impl FnMut(char) -> Option<()>,
) -> Option<bool> {
    active(budget)?;
    let name = name.split(|byte| *byte == b'.').next().unwrap_or_default();
    let mut complete = !name.is_empty();
    for component in name.split(|byte| *byte == b'_') {
        active(budget)?;
        complete &= visit_component(budget, component, zapf, &mut emit)?;
    }
    active(budget)?;
    Some(complete)
}

/// Zapf固有表・AGL・uni/u形式の優先順を守り、未知部分を推測しない。
fn visit_component(
    budget: &DecodeBudget,
    name: &[u8],
    zapf: bool,
    emit: &mut impl FnMut(char) -> Option<()>,
) -> Option<bool> {
    let known = zapf
        .then(|| lookup(tables::ZAPF, name))
        .flatten()
        .or_else(|| lookup(tables::AGL, name));
    if let Some(value) = known {
        for scalar in value.chars() {
            active(budget)?;
            emit(scalar)?;
        }
        return Some(true);
    }
    if let Some(hex) = name.strip_prefix(b"uni") {
        if hex.is_empty() || !hex.len().is_multiple_of(4) {
            return Some(false);
        }
        // component全体が有効な場合だけ通知し、後半surrogateで部分Unicodeを残さない。
        for group in hex.chunks_exact(4) {
            active(budget)?;
            if scalar(group).is_none() {
                return Some(false);
            }
        }
        for group in hex.chunks_exact(4) {
            active(budget)?;
            emit(scalar(group)?)?;
        }
        return Some(true);
    }
    if let Some(hex) = name.strip_prefix(b"u")
        && (4..=6).contains(&hex.len())
        && let Some(value) = scalar(hex)
    {
        emit(value)?;
        return Some(true);
    }
    Some(false)
}

/// 静的ソート済み表を二分探索し、最初の利用時にもheapを確保しない。
fn lookup(table: &'static [(&str, &str)], name: &[u8]) -> Option<&'static str> {
    let index = table
        .binary_search_by(|(key, _)| key.as_bytes().cmp(name))
        .ok()?;
    Some(table[index].1)
}

/// 大文字hexだけをUnicode scalarとして解釈し、surrogateや上限超過を拒否する。
fn scalar(hex: &[u8]) -> Option<char> {
    if hex.is_empty()
        || !hex
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(byte))
    {
        return None;
    }
    let value = hex.iter().try_fold(0u32, |value, byte| {
        let digit = if byte.is_ascii_digit() {
            byte - b'0'
        } else {
            byte - b'A' + 10
        };
        value.checked_mul(16)?.checked_add(u32::from(digit))
    })?;
    char::from_u32(value)
}

/// componentとscalarの有限な走査中も、共有した停止要求を確認する。
fn active(budget: &DecodeBudget) -> Option<()> {
    (!budget.is_stopped()).then_some(())
}

#[cfg(test)]
#[path = "glyph_names_tests.rs"]
mod tests;
