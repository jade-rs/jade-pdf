//! ToUnicode の entry 走査と範囲展開を累積予算内で行う。
use super::ToUnicodeCMapData;
use crate::decode::DecodeBudget;
use std::fmt::Write;

/// byte slice の CMap を読み、反復処理を開始する前に共通予算を予約する。
pub(super) fn parse_tounicode_cmap_from_bytes(
    budget: &DecodeBudget,
    bytes: &[u8],
) -> ToUnicodeCMapData {
    let mut cmap = ToUnicodeCMapData::default();
    let Some(source) = super::cmap_input::cmap_text(budget, bytes) else {
        return cmap;
    };
    let mut in_bfchar = false;
    let mut in_bfrange = false;
    let mut in_codespace_range = false;
    for line in source.lines() {
        if budget.is_stopped() {
            break;
        }
        let trimmed = line.trim();
        if trimmed.ends_with("begincodespacerange") {
            in_codespace_range = true;
            continue;
        }
        if trimmed.ends_with("endcodespacerange") {
            in_codespace_range = false;
            continue;
        }
        if trimmed.ends_with("beginbfchar") {
            in_bfchar = true;
            continue;
        }
        if trimmed.ends_with("endbfchar") {
            in_bfchar = false;
            continue;
        }
        if trimmed.ends_with("beginbfrange") {
            in_bfrange = true;
            continue;
        }
        if trimmed.ends_with("endbfrange") {
            in_bfrange = false;
            continue;
        }
        let mut hexes = HexTokens::new(trimmed);
        let (Some(first), Some(second)) = (hexes.next(), hexes.next()) else {
            continue;
        };
        if in_codespace_range {
            if budget.charge_tounicode_entries(1).is_none() {
                break;
            }
            if cmap
                .insert_code_space_range(budget, first, second)
                .is_none()
            {
                break;
            }
        } else if in_bfchar {
            if budget.charge_tounicode_entries(1).is_none() {
                break;
            }
            if insert_utf16_mapping(budget, &mut cmap, MappingKey::Hex(first), second).is_none() {
                break;
            }
        } else if in_bfrange {
            let Some(start) = u32::from_str_radix(first, 16).ok() else {
                continue;
            };
            let Some(end) = u32::from_str_radix(second, 16).ok() else {
                continue;
            };
            if start > end {
                continue;
            }
            let count = u64::from(end) - u64::from(start) + 1;
            let width = first.len();
            if trimmed.contains('[') {
                let count = count.min(hexes.clone().count() as u64);
                if budget.charge_tounicode_entries(count).is_none() {
                    break;
                }
                for (offset, dst_hex) in hexes.take(count as usize).enumerate() {
                    // count は source 範囲以下のため、u64 の和は u32 に収まる。
                    let code = (u64::from(start) + offset as u64) as u32;
                    if insert_utf16_mapping(
                        budget,
                        &mut cmap,
                        MappingKey::Code(code, width),
                        dst_hex,
                    )
                    .is_none()
                    {
                        break;
                    }
                }
                continue;
            }
            let Some(dst_start) = hexes
                .next()
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            else {
                continue;
            };
            if budget.charge_tounicode_entries(count).is_none() {
                break;
            }
            if u64::from(dst_start) + count - 1 > u64::from(u32::MAX) {
                budget.invalid_tounicode("bfrange destination exceeds u32");
                break;
            }
            for code in start..=end {
                let dst = dst_start + (code - start);
                if let Some(ch) = char::from_u32(dst) {
                    let key = MappingKey::Code(code, width);
                    if budget
                        .charge_tounicode_bytes(
                            (key.len() as u64).saturating_add(ch.len_utf8() as u64),
                        )
                        .is_none()
                    {
                        break;
                    }
                    if cmap.reserve_mapping(budget).is_none() {
                        break;
                    }
                    let Some(key) = key.allocate(budget) else {
                        break;
                    };
                    let Some(mut value) = budget.try_string_capacity(ch.len_utf8()) else {
                        break;
                    };
                    value.push(ch);
                    cmap.insert_mapping(key, value);
                }
            }
        }
    }
    cmap
}

/// allocation 前に桁幅を決定できる mapping key。
enum MappingKey<'a> {
    /// bfchar の入力 hex。奇数桁は末尾ゼロで偶数化する。
    Hex(&'a str),
    /// bfrange の source code と最小桁幅。
    Code(u32, usize),
}

impl MappingKey<'_> {
    /// 正規化後の key の正確な byte 数を返す。
    fn len(&self) -> usize {
        match self {
            Self::Hex(hex) => hex.len() + hex.len() % 2,
            Self::Code(code, width) => {
                (*width).max((32 - code.leading_zeros()).max(1).div_ceil(4) as usize)
            }
        }
    }

    /// 予約済みの容量へ直接 key を書き、途中の String を作らない。
    fn allocate(&self, budget: &DecodeBudget) -> Option<String> {
        let mut key = budget.try_string_capacity(self.len())?;
        match self {
            Self::Hex(hex) => {
                key.extend(hex.bytes().map(|byte| byte.to_ascii_uppercase() as char));
                if !hex.len().is_multiple_of(2) {
                    key.push('0');
                }
            }
            Self::Code(code, width) => {
                write!(&mut key, "{code:0width$X}").ok()?;
            }
        }
        Some(key)
    }
}

/// 検証済み ASCII hex を一時配列なしで UTF-16 復号し、先頭 BOM を除く。
fn utf16_chars(hex: &str) -> impl Iterator<Item = Result<char, std::char::DecodeUtf16Error>> + '_ {
    let units = hex.as_bytes().chunks_exact(4).map(|digits| {
        digits.iter().fold(0u16, |value, digit| {
            (value << 4) | (*digit as char).to_digit(16).unwrap_or(0) as u16
        })
    });
    char::decode_utf16(units)
        .enumerate()
        .filter_map(|(index, ch)| {
            if index == 0 && ch == Ok('\u{feff}') {
                None
            } else {
                Some(ch)
            }
        })
}

/// UTF-8 payload を借用走査で数え、両 String の確保前に一括予約する。
fn insert_utf16_mapping(
    budget: &DecodeBudget,
    cmap: &mut ToUnicodeCMapData,
    key: MappingKey<'_>,
    hex: &str,
) -> Option<()> {
    let Some(length) =
        utf16_chars(hex).try_fold(0usize, |length, ch| Some(length + ch.ok()?.len_utf8()))
    else {
        return Some(());
    };
    budget.charge_tounicode_bytes((key.len() as u64).saturating_add(length as u64))?;
    cmap.reserve_mapping(budget)?;
    let key = key.allocate(budget)?;
    let mut value = budget.try_string_capacity(length)?;
    for ch in utf16_chars(hex) {
        value.push(ch.ok()?);
    }
    cmap.insert_mapping(key, value);
    Some(())
}

/// 一行内の hex operand を allocation なしで借用する iterator。
#[derive(Clone)]
struct HexTokens<'a> {
    /// UTF-8 境界を保った入力行。
    line: &'a str,
    /// 次の operand を探す byte 位置。
    cursor: usize,
}

impl<'a> HexTokens<'a> {
    /// 行の先頭から借用走査を始める。
    fn new(line: &'a str) -> Self {
        Self { line, cursor: 0 }
    }
}

impl<'a> Iterator for HexTokens<'a> {
    type Item = &'a str;

    /// 有効な hex を借用し、不正 token は従来と同じく読み飛ばす。
    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.line.as_bytes();
        while self.cursor < bytes.len() {
            let position = self.cursor;
            self.cursor += 1;
            if bytes[position] != b'<' || bytes.get(position + 1) == Some(&b'<') {
                continue;
            }
            let start = self.cursor;
            let Some(length) = bytes[start..].iter().position(|byte| *byte == b'>') else {
                self.cursor = bytes.len();
                return None;
            };
            let end = start + length;
            self.cursor = end + 1;
            let hex = &self.line[start..end];
            if !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Some(hex);
            }
        }
        None
    }
}
