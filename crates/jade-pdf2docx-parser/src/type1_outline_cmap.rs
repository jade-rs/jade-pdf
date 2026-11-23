//! 確認済みのUnicode/glyph対応を、BMPと全平面の二形式へ格納する。
use crate::decode::DecodeBudget;

/// 一つのUnicode scalarと、元の外形を指すGID。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Mapping {
    /// surrogateを含まないUnicode scalar。
    pub(super) scalar: u32,
    /// 0の.notdefとは区別した、既存glyphのGID。
    pub(super) glyph: u16,
}

/// format4/12をUnicode/Windows両platformで共有し、曖昧な衝突は拒否する。
pub(super) fn encode(
    budget: &DecodeBudget,
    mappings: &[Mapping],
    glyph_count: u16,
) -> Option<Vec<u8>> {
    if glyph_count == 0 || mappings.len() > 65535 || budget.is_stopped() {
        return None;
    }
    for mapping in mappings {
        if budget.is_stopped()
            || char::from_u32(mapping.scalar).is_none()
            || mapping.glyph == 0
            || mapping.glyph >= glyph_count
        {
            return None;
        }
    }
    let mut normalized = budget.font_vector(mappings.len())?;
    normalized.extend_from_slice(mappings);
    normalized.sort_unstable_by_key(|m| m.scalar);
    if normalized
        .windows(2)
        .any(|pair| pair[0].scalar == pair[1].scalar && pair[0].glyph != pair[1].glyph)
    {
        return None;
    }
    normalized.dedup_by_key(|m| m.scalar);
    let mut bmp = budget.font_vector(normalized.len() + 1)?;
    let mut full = budget.font_vector(normalized.len())?;
    for &mapping in &normalized {
        if budget.is_stopped() {
            return None;
        }
        append(&mut full, mapping)?;
        if mapping.scalar < 65535 {
            append(&mut bmp, mapping)?;
        }
    }
    bmp.push(Group {
        start: 65535,
        end: 65535,
        glyph: 0,
    });
    let length4 = u16::try_from(16_usize.checked_add(bmp.len().checked_mul(8)?)?).ok()?;
    let length12 = 16_usize.checked_add(full.len().checked_mul(12)?)?;
    let total = 36_usize
        .checked_add(usize::from(length4))?
        .checked_add(length12)?;
    if total > crate::parser::MAX_EMBEDDED_FONT_BYTES {
        return None;
    }
    let mut output = budget.font_vector(total)?;
    put16(&mut output, 0);
    put16(&mut output, 4);
    for (platform, encoding, offset) in [
        (0, 3, 36),
        (0, 4, 36 + usize::from(length4)),
        (3, 1, 36),
        (3, 10, 36 + usize::from(length4)),
    ] {
        put16(&mut output, platform);
        put16(&mut output, encoding);
        put32(&mut output, u32::try_from(offset).ok()?);
    }
    let count = u16::try_from(bmp.len()).ok()?;
    let power = 1_usize << (usize::BITS - bmp.len().leading_zeros() - 1);
    for value in [
        4,
        length4,
        0,
        count.checked_mul(2)?,
        (power * 2) as u16,
        power.trailing_zeros() as u16,
        ((bmp.len() - power) * 2) as u16,
    ] {
        put16(&mut output, value);
    }
    for group in &bmp {
        if budget.is_stopped() {
            return None;
        }
        put16(&mut output, group.end as u16);
    }
    put16(&mut output, 0);
    for group in &bmp {
        put16(&mut output, group.start as u16);
    }
    for group in &bmp {
        put16(
            &mut output,
            (group.glyph as u16).wrapping_sub(group.start as u16),
        );
    }
    for _ in &bmp {
        put16(&mut output, 0);
    }
    put16(&mut output, 12);
    put16(&mut output, 0);
    for value in [
        u32::try_from(length12).ok()?,
        0,
        u32::try_from(full.len()).ok()?,
    ] {
        put32(&mut output, value);
    }
    for group in full {
        if budget.is_stopped() {
            return None;
        }
        for value in [group.start, group.end, group.glyph] {
            put32(&mut output, value);
        }
    }
    (!budget.is_stopped()).then_some(output)
}

/// UnicodeとGIDがともに連続する、固定長の対応範囲。
struct Group {
    /// 最初のUnicode scalar。
    start: u32,
    /// 最後のUnicode scalar。
    end: u32,
    /// 最初のGID。範囲内の差分だけ増やす。
    glyph: u32,
}

/// 既に予約した範囲配列へ追加し、同じdeltaで連続する対応だけを併合する。
fn append(groups: &mut Vec<Group>, mapping: Mapping) -> Option<()> {
    if let Some(last) = groups.last_mut()
        && last.end.checked_add(1) == Some(mapping.scalar)
        && last.glyph.checked_add(last.end - last.start + 1) == Some(u32::from(mapping.glyph))
    {
        last.end = mapping.scalar;
        return Some(());
    }
    if groups.len() == groups.capacity() {
        return None;
    }
    groups.push(Group {
        start: mapping.scalar,
        end: mapping.scalar,
        glyph: u32::from(mapping.glyph),
    });
    Some(())
}

/// 予約済み領域へ大端16bitを格納する。
fn put16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}
/// 予約済み領域へ大端32bitを格納する。
fn put32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
#[path = "type1_outline_cmap_tests.rs"]
mod tests;
