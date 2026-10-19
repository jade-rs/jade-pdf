//! CCITTの行境界・終端・追加行の資源予約を共通管理する。
use super::{DecodeBudget, Parameters, bits::InputBits};

/// 終端・宣言行数・正常な入力末尾を区別し、各行の追加前に資源予算を検査する。
pub(super) fn decode(
    rgb: &mut Vec<u8>,
    data: &[u8],
    parameters: Parameters,
    budget: Option<&DecodeBudget>,
    row: impl Fn(&mut Vec<u8>, &mut InputBits<'_>, &Parameters) -> Option<()>,
    terminal: impl Fn(&mut InputBits<'_>, &Parameters) -> Option<()>,
) -> Option<u32> {
    let mut bits = InputBits::new(data)?;
    let mut rows = 0u32;
    let mut damaged = 0u32;
    let mut previous_damaged = false;
    let row_bytes = usize::from(parameters.columns).checked_mul(3)?;
    loop {
        if !parameters.end_of_block && parameters.rows > 0 && rows == parameters.rows {
            return Some(damaged);
        }
        let mut probe = bits.clone();
        if terminal(&mut probe, &parameters).is_some() {
            return Some(damaged);
        }
        if !parameters.end_of_block && bits.padded_end() {
            return Some(damaged);
        }
        if parameters.byte_aligned {
            bits.align()?;
        }
        if parameters.end_of_line || bits.exact(9) == Some(0) {
            bits.eol()?;
            // 有効な最終EOLだけでは新しい画像行を作らない。
            if !parameters.end_of_block && bits.padded_end() {
                return Some(damaged);
            }
        }
        rows = rows.checked_add(1)?;
        if let Some(budget) = budget {
            budget.check_image_pixels(u32::from(parameters.columns), rows)?;
        }
        let required = rgb.len().checked_add(row_bytes)?;
        if required > rgb.capacity() {
            // 再確保では新容量全体を累積予算へ加算し、古い配列も確保後まで保持する。
            let capacity = rgb.capacity().checked_mul(2)?.max(required);
            let mut grown = budget?.image_vector(capacity)?;
            grown.extend_from_slice(rgb);
            *rgb = grown;
        }
        let start = rgb.len();
        let mut row_start = bits.clone();
        let decoded = row(rgb, &mut bits, &parameters).is_some();
        let at_row_limit =
            !parameters.end_of_block && parameters.rows > 0 && rows == parameters.rows;
        if decoded
            && (!parameters.end_of_line
                || parameters.damaged_rows == 0
                || at_row_limit
                || (!parameters.end_of_block && bits.padded_end())
                || next_eol(&bits, &parameters))
        {
            previous_damaged = false;
            continue;
        }
        if damaged >= parameters.damaged_rows {
            return None;
        }
        row_start.find_eol(parameters.byte_aligned, budget)?;
        bits = row_start;
        // 部分的に復号したrunを捨て、直前の正常行または白行で置き換える。
        rgb.truncate(start);
        if start >= row_bytes && !previous_damaged {
            rgb.extend_from_within(start - row_bytes..start);
        } else {
            rgb.resize(required, if parameters.black_is_one { 0 } else { 255 });
        }
        damaged = damaged.checked_add(1)?;
        previous_damaged = true;
    }
}

/// 正常行の後ろが次のEOLへ接続するか、元readerを進めず検査する。
fn next_eol(bits: &InputBits<'_>, parameters: &Parameters) -> bool {
    let mut probe = bits.clone();
    (!parameters.byte_aligned || probe.align().is_some()) && probe.eol().is_some()
}
