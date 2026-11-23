//! 完成した各tableを、整列したOpenType-CFF directoryとchecksumでまとめる。
use crate::decode::DecodeBudget;

/// 固有tagで識別した、完成済みの借用table。
pub(super) struct Table<'a> {
    /// 四byteのOpenType tag。
    pub(super) tag: [u8; 4],
    /// 元のtable bytes。headのchecksum調整値だけは出力時に再計算する。
    pub(super) bytes: &'a [u8],
}

/// 最大32tableをtag順に並べ、4byte整列とfont全体のchecksumを確定する。
pub(super) fn pack(budget: &DecodeBudget, tables: &[Table<'_>]) -> Option<Vec<u8>> {
    let count = tables.len();
    if count == 0 || count > 32 || budget.is_stopped() {
        return None;
    }
    let mut order = [0_usize; 32];
    for (index, slot) in order[..count].iter_mut().enumerate() {
        *slot = index;
    }
    order[..count].sort_unstable_by_key(|&index| tables[index].tag);
    let mut offsets = [0_u32; 32];
    let mut checksums = [0_u32; 32];
    let mut total = 12 + 16 * count;
    let mut head = None;
    for (position, &index) in order[..count].iter().enumerate() {
        let table = &tables[index];
        if table.tag.iter().any(|byte| !(32..=126).contains(byte))
            || (position > 0 && tables[order[position - 1]].tag == table.tag)
        {
            return None;
        }
        let is_head = table.tag == *b"head";
        if is_head {
            if table.bytes.len() < 54
                || table.bytes.get(..4) != Some([0, 1, 0, 0].as_slice())
                || table.bytes.get(12..16) != Some([0x5f, 0x0f, 0x3c, 0xf5].as_slice())
            {
                return None;
            }
            head = Some(total);
        }
        offsets[position] = u32::try_from(total).ok()?;
        u32::try_from(table.bytes.len()).ok()?;
        total = total.checked_add(
            table
                .bytes
                .len()
                .checked_add((4 - table.bytes.len() % 4) % 4)?,
        )?;
        if total > crate::parser::MAX_EMBEDDED_FONT_BYTES {
            return None;
        }
        checksums[position] = checksum(budget, table.bytes, is_head)?;
    }
    let head = head?;
    let mut output = budget.font_vector(total)?;
    output.extend_from_slice(b"OTTO");
    let power = 1_usize << (usize::BITS - count.leading_zeros() - 1);
    for value in [
        count as u16,
        (power * 16) as u16,
        power.trailing_zeros() as u16,
        ((count - power) * 16) as u16,
    ] {
        output.extend_from_slice(&value.to_be_bytes());
    }
    for (position, &index) in order[..count].iter().enumerate() {
        let table = &tables[index];
        output.extend_from_slice(&table.tag);
        for value in [
            checksums[position],
            offsets[position],
            table.bytes.len() as u32,
        ] {
            output.extend_from_slice(&value.to_be_bytes());
        }
    }
    for &index in &order[..count] {
        if budget.is_stopped() {
            return None;
        }
        let table = &tables[index];
        let start = output.len();
        output.extend_from_slice(table.bytes);
        if table.tag == *b"head" {
            output[start + 8..start + 12].fill(0);
        }
        output.resize(
            output.len().checked_add((4 - table.bytes.len() % 4) % 4)?,
            0,
        );
    }
    let adjustment = 0xb1b0afba_u32.wrapping_sub(checksum(budget, &output, false)?);
    output[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    (!budget.is_stopped()).then_some(output)
}

/// 不足byteを0で補い、head自身のchecksum計算では旧調整値を除外する。
fn checksum(budget: &DecodeBudget, bytes: &[u8], zero_adjustment: bool) -> Option<u32> {
    let mut sum = 0_u32;
    for (index, chunk) in bytes.chunks(4).enumerate() {
        if index.is_multiple_of(1024) && budget.is_stopped() {
            return None;
        }
        if zero_adjustment && index == 2 {
            continue;
        }
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    (!budget.is_stopped()).then_some(sum)
}

#[cfg(test)]
#[path = "type1_outline_sfnt_tests.rs"]
mod tests;
