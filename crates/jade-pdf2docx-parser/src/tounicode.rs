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
    let mut mode = Mode::None;
    let mut tokens = tokens::Tokens::new(&source).peekable();
    while let Some(token) = tokens.next() {
        if budget.check().is_err() {
            break;
        }
        match token {
            tokens::Token::Word(word) => match word {
                b"begincodespacerange" => mode = Mode::CodeSpace,
                b"beginbfchar" => mode = Mode::Char,
                b"beginbfrange" => mode = Mode::Range,
                b"endcodespacerange" | b"endbfchar" | b"endbfrange" => mode = Mode::None,
                _ => {}
            },
            tokens::Token::Hex(first) if mode != Mode::None => {
                let second = match tokens.peek().copied() {
                    Some(tokens::Token::Hex(value)) => {
                        tokens.next();
                        value
                    }
                    _ => continue,
                };
                if mode == Mode::Range {
                    if range(budget, &mut cmap, first, second, &mut tokens).is_none() {
                        break;
                    }
                    continue;
                }
                let (Some(first), Some(second)) = (first, second) else {
                    continue;
                };
                if first.is_empty() {
                    continue;
                }
                if budget.charge_tounicode_entries(1).is_none() {
                    break;
                }
                let result = if mode == Mode::CodeSpace {
                    cmap.insert_code_space_range(budget, first, second)
                } else {
                    insert_utf16_mapping(budget, &mut cmap, MappingKey::Hex(first), second)
                };
                if result.is_none() {
                    break;
                }
            }
            _ => {}
        }
    }
    cmap
}

#[path = "tounicode_tokens.rs"]
mod tokens;

/// 対応するCMapのrecord種別。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// 有効なmapping blockの外側。
    None,
    /// source codeの範囲。
    CodeSpace,
    /// sourceと宛先のpair。
    Char,
    /// source範囲と宛先scalarまたは配列。
    Range,
}

/// 有限のsource範囲と元のhex桁幅を確認する。
fn source_range(first: Option<&str>, second: Option<&str>) -> Option<(u32, u32, usize)> {
    let first = first.filter(|s| !s.is_empty())?;
    let start = u32::from_str_radix(first, 16).ok()?;
    let end = u32::from_str_radix(second?, 16).ok()?;
    (start <= end).then_some((start, end, first.len()))
}

/// 配列の全体を閉じるまで読んでから、反復数を予約してmappingを作る。
fn range<'a>(
    budget: &DecodeBudget,
    cmap: &mut ToUnicodeCMapData,
    first: Option<&'a str>,
    second: Option<&'a str>,
    tokens: &mut std::iter::Peekable<tokens::Tokens<'a>>,
) -> Option<()> {
    let bounds = source_range(first, second);
    match tokens.peek().copied() {
        Some(tokens::Token::Hex(destination)) => {
            tokens.next();
            if let (Some((start, end, width)), Some(destination)) = (bounds, destination) {
                scalar_range(budget, cmap, start, end, width, destination)?;
            }
        }
        Some(tokens::Token::Open(false)) => {
            tokens.next();
            let beginning = tokens.clone();
            let mut scan = tokens::ArrayHex::new(tokens.clone(), budget);
            let available = scan.by_ref().count() as u64;
            let closed = scan.closed;
            *tokens = scan.tokens;
            if budget.is_stopped() {
                return None;
            }
            if let (true, Some((start, end, width))) = (closed, bounds) {
                let count = (u64::from(end) - u64::from(start) + 1).min(available);
                budget.charge_tounicode_entries(count)?;
                for (offset, destination) in tokens::ArrayHex::new(beginning, budget)
                    .take(count as usize)
                    .enumerate()
                {
                    let code = (u64::from(start) + offset as u64) as u32;
                    insert_utf16_mapping(budget, cmap, MappingKey::Code(code, width), destination)?;
                }
            }
        }
        _ => {}
    }
    Some(())
}

/// scalarの既存の加算規則と予算予約を保ち、overflow・取消しを先に確認する。
fn scalar_range(
    budget: &DecodeBudget,
    cmap: &mut ToUnicodeCMapData,
    start: u32,
    end: u32,
    width: usize,
    destination: &str,
) -> Option<()> {
    let Ok(dst_start) = u32::from_str_radix(destination, 16) else {
        return Some(());
    };
    let count = u64::from(end) - u64::from(start) + 1;
    budget.charge_tounicode_entries(count)?;
    if u64::from(dst_start) + count - 1 > u64::from(u32::MAX) {
        budget.invalid_tounicode("bfrange destination exceeds u32");
        return None;
    }
    for code in start..=end {
        budget.check().ok()?;
        let dst = dst_start + (code - start);
        if let Some(ch) = char::from_u32(dst) {
            let key = MappingKey::Code(code, width);
            budget
                .charge_tounicode_bytes((key.len() as u64).saturating_add(ch.len_utf8() as u64))?;
            cmap.reserve_mapping(budget)?;
            let key = key.allocate(budget)?;
            let mut value = budget.try_string_capacity(ch.len_utf8())?;
            value.push(ch);
            cmap.insert_mapping(key, value);
        }
    }
    Some(())
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

/// 完全なUTF16となるASCII hexを借用復号し、奇数桁の末尾ゼロと先頭BOMを扱う。
fn utf16_chars(hex: &str) -> impl Iterator<Item = Result<char, std::char::DecodeUtf16Error>> + '_ {
    let units = hex.as_bytes().chunks(4).map(|digits| {
        let unit = digits.iter().fold(0u16, |value, digit| {
            (value << 4) | (*digit as char).to_digit(16).unwrap_or(0) as u16
        });
        unit << (4 * (4 - digits.len()))
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
    if !hex.len().div_ceil(2).is_multiple_of(2) {
        return Some(());
    }
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
