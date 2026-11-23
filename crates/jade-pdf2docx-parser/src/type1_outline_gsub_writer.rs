//! 小さい16bit参照領域と大きい合字内容を分離し、検査済みGSUBを一度だけ確保する。
use super::{DecodeBudget, Rule};

/// required rligを持つDFLT scriptと、type7を経由するtype4合字を格納する。
pub(super) fn encode(budget: &DecodeBudget, order: &[&Rule<'_>], size: usize) -> Option<Vec<u8>> {
    let count = order.len();
    let mut output = budget.font_vector(size)?;
    words(&mut output, &[1, 0, 10, 28, 42]);
    words(&mut output, &[1]);
    output.extend_from_slice(b"DFLT");
    words(&mut output, &[8, 4, 0, 0, 0, 0]);
    words(&mut output, &[1]);
    output.extend_from_slice(b"rlig");
    words(&mut output, &[8, 0, 1, 0]);
    words(&mut output, &[1, 4, 7, 0, u16::try_from(count).ok()?]);
    for index in 0..count {
        words(
            &mut output,
            &[u16::try_from(6 + 2 * count + 8 * index).ok()?],
        );
    }
    let extension_start = output.len();
    let mut subtable = extension_start.checked_add(8 * count)?;
    for (index, rule) in order.iter().enumerate() {
        if budget.is_stopped() {
            return None;
        }
        words(&mut output, &[1, 4]);
        let offset = subtable.checked_sub(extension_start + 8 * index)?;
        output.extend_from_slice(&u32::try_from(offset).ok()?.to_be_bytes());
        subtable = subtable.checked_add(20 + 2 * rule.components.len())?;
    }
    for rule in order {
        if budget.is_stopped() {
            return None;
        }
        words(
            &mut output,
            &[
                1,
                8,
                1,
                14,
                1,
                1,
                rule.components[0],
                1,
                4,
                rule.glyph,
                u16::try_from(rule.components.len()).ok()?,
            ],
        );
        for &component in &rule.components[1..] {
            if budget.is_stopped() {
                return None;
            }
            words(&mut output, &[component]);
        }
    }
    (output.len() == size && !budget.is_stopped()).then_some(output)
}

/// 予約済みbyte領域へ、big-endianの短い固定列を書き込む。
fn words(output: &mut Vec<u8>, values: &[u16]) {
    for &value in values {
        output.extend_from_slice(&value.to_be_bytes());
    }
}
