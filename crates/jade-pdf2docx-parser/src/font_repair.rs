//! 埋め込み sfnt を借用し、予算内の配列だけで Unicode cmap を修復する。
use crate::decode::DecodeBudget;

/// 同じ Unicode へ複数 source が対応する場合の先勝ち順序を保持する。
#[derive(Clone, Copy)]
struct Glyph {
    /// 修復対象の BMP scalar。
    unicode: u16,
    /// 元の format 6 にある glyph ID。
    id: u16,
    /// 元の ToUnicode での走査順。
    order: usize,
}

/// format 6 の glyph 配列をコピーせず参照する。
struct Format6<'a> {
    /// 先頭の文字 code。
    first: u16,
    /// 検査済み glyph ID の big-endian 配列。
    glyphs: &'a [u8],
}

impl Format6<'_> {
    /// 文字 code の範囲と配列境界を検査して glyph を読む。
    fn glyph(&self, code: u16) -> Option<u16> {
        read_be_u16(self.glyphs, usize::from(code.checked_sub(self.first)?) * 2)
    }
}

/// format 6 と ToUnicode を合成し、最終 Font と修復配列を累積予算へ数える。
pub(crate) fn repair_sfnt_unicode_cmap<I>(
    budget: &DecodeBudget,
    bytes: &[u8],
    mappings: I,
) -> Option<Vec<u8>>
where
    I: Iterator<Item = (u16, u16)> + Clone,
{
    if budget.is_stopped() {
        return None;
    }
    let codes = sfnt_format6(bytes)?;
    let mut glyphs = budget.font_vector::<Glyph>(mappings.clone().count())?;
    for (order, (source, unicode)) in mappings.enumerate() {
        let id = codes.glyph(source)?;
        if id != 0 {
            glyphs.push(Glyph { unicode, id, order });
        }
    }
    glyphs.sort_unstable_by_key(|glyph| (glyph.unicode, glyph.order));
    glyphs.dedup_by_key(|glyph| glyph.unicode);
    if glyphs.is_empty() || glyphs.len() > 4095 {
        return None;
    }
    let cmap = build_unicode_bmp_cmap(budget, &glyphs)?;
    rebuild_sfnt_with_table(budget, bytes, *b"cmap", &cmap)
}

/// format 6 の連続した code/glyph 領域を境界検査して借用する。
fn sfnt_format6(bytes: &[u8]) -> Option<Format6<'_>> {
    let cmap = sfnt_table(bytes, b"cmap")?;
    let record_count = usize::from(read_be_u16(cmap, 2)?);
    for index in 0..record_count {
        let record = 4 + index * 8;
        let subtable_offset = read_be_u32(cmap, record + 4)? as usize;
        let subtable = cmap.get(subtable_offset..)?;
        if read_be_u16(subtable, 0)? != 6 {
            continue;
        }
        let first = read_be_u16(subtable, 6)?;
        let count = read_be_u16(subtable, 8)?;
        if count != 0 {
            first.checked_add(count - 1)?;
        }
        let glyphs = subtable.get(10..10 + usize::from(count) * 2)?;
        return Some(Format6 { first, glyphs });
    }
    None
}

/// sfnt directory から指定 table を借用する。
fn sfnt_table<'a>(bytes: &'a [u8], expected_tag: &[u8; 4]) -> Option<&'a [u8]> {
    let num_tables = usize::from(read_be_u16(bytes, 4)?);
    for index in 0..num_tables {
        let record = 12 + index * 16;
        if bytes.get(record..record + 4)? != expected_tag {
            continue;
        }
        let offset = read_be_u32(bytes, record + 8)? as usize;
        let length = read_be_u32(bytes, record + 12)? as usize;
        return bytes.get(offset..offset.checked_add(length)?);
    }
    None
}

/// format 4 を二つの encoding record から共有し、一つの配列に直接書く。
fn build_unicode_bmp_cmap(budget: &DecodeBudget, mappings: &[Glyph]) -> Option<Vec<u8>> {
    let segment_count = mappings.len().checked_add(1)?;
    let segment_count_x2 = u16::try_from(segment_count).ok()?.checked_mul(2)?;
    let power = 1usize << (usize::BITS - segment_count.leading_zeros() - 1);
    let search_range = u16::try_from(power * 2).ok()?;
    let entry_selector = power.trailing_zeros() as u16;
    let range_shift = segment_count_x2.checked_sub(search_range)?;
    let format_length = u16::try_from(16usize.checked_add(segment_count.checked_mul(8)?)?).ok()?;
    let mut cmap = budget.font_vector(20 + usize::from(format_length))?;
    for value in [0, 2, 0, 3] {
        push_be_u16(&mut cmap, value);
    }
    push_be_u32(&mut cmap, 20);
    push_be_u16(&mut cmap, 3);
    push_be_u16(&mut cmap, 1);
    push_be_u32(&mut cmap, 20);
    for value in [
        4,
        format_length,
        0,
        segment_count_x2,
        search_range,
        entry_selector,
        range_shift,
    ] {
        push_be_u16(&mut cmap, value);
    }
    for glyph in mappings {
        push_be_u16(&mut cmap, glyph.unicode);
    }
    push_be_u16(&mut cmap, 0xffff);
    push_be_u16(&mut cmap, 0);
    for glyph in mappings {
        push_be_u16(&mut cmap, glyph.unicode);
    }
    push_be_u16(&mut cmap, 0xffff);
    for glyph in mappings {
        push_be_u16(&mut cmap, glyph.id.wrapping_sub(glyph.unicode));
    }
    push_be_u16(&mut cmap, 1);
    for _ in 0..segment_count {
        push_be_u16(&mut cmap, 0);
    }
    Some(cmap)
}

/// 元 table または置換 table を借用し、directory の値を保持する。
struct Table<'a> {
    /// 四文字の table 識別子。
    tag: [u8; 4],
    /// コピーせず借用する table payload。
    data: &'a [u8],
    /// 同じ tag の元の順序を保持する。
    order: usize,
    /// 再構築後の先頭 offset。
    offset: u32,
}

/// table の複製を作らず、directory と payload を最終 Font 配列へ直接書く。
fn rebuild_sfnt_with_table(
    budget: &DecodeBudget,
    bytes: &[u8],
    replacement_tag: [u8; 4],
    replacement: &[u8],
) -> Option<Vec<u8>> {
    let signature = bytes.get(..4)?;
    if signature == b"ttcf" {
        return None;
    }
    let num_tables = read_be_u16(bytes, 4)?;
    let mut tables = budget.font_vector::<Table<'_>>(usize::from(num_tables))?;
    for index in 0..usize::from(num_tables) {
        let record = 12 + index * 16;
        let tag: [u8; 4] = bytes.get(record..record + 4)?.try_into().ok()?;
        let offset = read_be_u32(bytes, record + 8)? as usize;
        let length = read_be_u32(bytes, record + 12)? as usize;
        let original = bytes.get(offset..offset.checked_add(length)?)?;
        let data = if tag == replacement_tag {
            replacement
        } else {
            original
        };
        if tag == *b"head" && data.len() < 12 {
            return None;
        }
        tables.push(Table {
            tag,
            data,
            order: index,
            offset: 0,
        });
    }
    if !tables.iter().any(|table| table.tag == replacement_tag)
        || !tables.iter().any(|table| table.tag == *b"head")
    {
        return None;
    }
    tables.sort_unstable_by_key(|table| (table.tag, table.order));
    let power = 1usize << (usize::BITS - usize::from(num_tables).leading_zeros() - 1);
    let search_range = u16::try_from(power * 16).ok()?;
    let entry_selector = power.trailing_zeros() as u16;
    let range_shift = num_tables.checked_mul(16)?.checked_sub(search_range)?;
    let mut output_length = 12 + usize::from(num_tables) * 16;
    for table in &mut tables {
        table.offset = u32::try_from(output_length).ok()?;
        u32::try_from(table.data.len()).ok()?;
        output_length = output_length.checked_add(align_to_four(table.data.len())?)?;
    }
    let mut output = budget.font_vector(output_length)?;
    output.extend_from_slice(signature);
    for value in [num_tables, search_range, entry_selector, range_shift] {
        push_be_u16(&mut output, value);
    }
    for table in &tables {
        output.extend_from_slice(&table.tag);
        let checksum = if table.tag == *b"head" {
            sfnt_checksum(&table.data[..8]).wrapping_add(sfnt_checksum(&table.data[12..]))
        } else {
            sfnt_checksum(table.data)
        };
        push_be_u32(&mut output, checksum);
        push_be_u32(&mut output, table.offset);
        push_be_u32(&mut output, table.data.len() as u32);
    }
    for table in &tables {
        let start = output.len();
        output.extend_from_slice(table.data);
        if table.tag == *b"head" {
            output[start + 8..start + 12].fill(0);
        }
        output.resize(output.len().checked_add((4 - table.data.len() % 4) % 4)?, 0);
    }
    let head_offset = tables.iter().find(|table| table.tag == *b"head")?.offset as usize;
    let adjustment = 0xb1b0_afbau32.wrapping_sub(sfnt_checksum(&output));
    output[head_offset + 8..head_offset + 12].copy_from_slice(&adjustment.to_be_bytes());
    Some(output)
}

/// sfnt checksum 計算用に byte 長を四の倍数へ切り上げる。
fn align_to_four(length: usize) -> Option<usize> {
    length.checked_add((4 - length % 4) % 4)
}

/// sfnt の32bit big-endian 加算 checksum を計算する。
fn sfnt_checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |checksum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        checksum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// Vec 末尾へ big-endian u16 を追加する。
fn push_be_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

/// Vec 末尾へ big-endian u32 を追加する。
fn push_be_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

/// byte 列から big-endian u16 を境界検査つきで読む。
fn read_be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// byte 列から big-endian u32 を境界検査つきで読む。
fn read_be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
#[path = "font_repair_tests.rs"]
mod tests;
